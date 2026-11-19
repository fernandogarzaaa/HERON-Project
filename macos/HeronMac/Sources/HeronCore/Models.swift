import Foundation

// Plain value types the views read. They are shaped after the core's API (heron-v1.md §3 and §4,
// src/store.rs, src/server.rs) so U4b can add decoding without touching a view. U4a decodes nothing.

/// The endpoint line printed by `heron-cli serve --print-endpoint`: `{port, token, background_owner, version}`.
public struct CoreEndpoint: Sendable, Equatable {
    public var port: Int
    public var token: String
    /// False when another `heron serve` already owns schedules and the phone connection.
    public var backgroundOwner: Bool
    public var version: String

    public init(port: Int, token: String, backgroundOwner: Bool, version: String) {
        self.port = port
        self.token = token
        self.backgroundOwner = backgroundOwner
        self.version = version
    }
}

/// `readonly`, `ask`, `auto` or `yolo` (src/permissions.rs `Mode::name`).
public enum PermissionMode: String, Sendable, CaseIterable, Equatable {
    case readonly, ask, auto, yolo

    public var title: String {
        switch self {
        case .readonly: "Read only"
        case .ask: "Ask first"
        case .auto: "Auto"
        case .yolo: "Don't ask"
        }
    }

    public var sentence: String {
        switch self {
        case .readonly: "Heron can look and search, and changes nothing."
        case .ask: "Heron asks before it writes, sends or runs anything."
        case .auto: "Heron does safe things on its own and asks before anything risky."
        case .yolo: "Heron does everything without asking. Only for a folder you can lose."
        }
    }
}

/// `GET /api/state`.
public struct CoreState: Sendable, Equatable {
    public var name: String
    public var version: String
    public var model: String
    public var mode: PermissionMode
    public var workspace: String
    public var paused: Bool
    public var waiting: Int
    public var unread: Int
    public var memories: Int
    public var onboarded: Bool
    public var riskNoticeAccepted: Bool
    /// The core serves the sandbox kind as a name: `seatbelt`, `bubblewrap` or `none`.
    public var sandbox: String
    public var hasKey: Bool

    /// On macOS the sandbox is on when the core reports anything but `none`.
    public var sandboxOn: Bool { sandbox != "none" }

    public init(name: String, version: String, model: String, mode: PermissionMode, workspace: String,
                paused: Bool, waiting: Int, unread: Int, memories: Int, onboarded: Bool,
                riskNoticeAccepted: Bool, sandbox: String, hasKey: Bool) {
        self.name = name; self.version = version; self.model = model; self.mode = mode
        self.workspace = workspace; self.paused = paused; self.waiting = waiting; self.unread = unread
        self.memories = memories; self.onboarded = onboarded; self.riskNoticeAccepted = riskNoticeAccepted
        self.sandbox = sandbox; self.hasKey = hasKey
    }
}

// MARK: Sessions

public struct SessionSummary: Sendable, Equatable, Identifiable {
    public var id: String
    public var title: String
    public var updatedAt: Date
    public var messages: Int

    public init(id: String, title: String, updatedAt: Date, messages: Int) {
        self.id = id
        self.title = title
        self.updatedAt = updatedAt
        self.messages = messages
    }
}

public struct ToolCallInfo: Sendable, Equatable, Identifiable {
    public var id: String
    public var name: String
    public var summary: String
    public var ok: Bool?
    public var durationMs: Int?
    /// True when the step read outside content (the core's untrusted-output flag).
    public var untrusted: Bool

    public init(id: String, name: String, summary: String, ok: Bool?, durationMs: Int?, untrusted: Bool) {
        self.id = id
        self.name = name
        self.summary = summary
        self.ok = ok
        self.durationMs = durationMs
        self.untrusted = untrusted
    }
}

public struct SessionMessage: Sendable, Equatable, Identifiable {
    public var id: Int
    /// `user` or `assistant`. Tool results are folded into `toolCalls`.
    public var role: String
    public var content: String
    public var toolCalls: [ToolCallInfo]

    public init(id: Int, role: String, content: String, toolCalls: [ToolCallInfo]) {
        self.id = id
        self.role = role
        self.content = content
        self.toolCalls = toolCalls
    }
}

/// `GET /api/sessions/{id}`.
public struct SessionDetail: Sendable, Equatable {
    public var id: String
    public var title: String
    public var messages: [SessionMessage]
    public var busy: Bool
    public var interrupted: Bool
    /// The conversation has read outside content (the taint flag).
    public var tainted: Bool

    public init(id: String, title: String, messages: [SessionMessage], busy: Bool, interrupted: Bool, tainted: Bool) {
        self.id = id
        self.title = title
        self.messages = messages
        self.busy = busy
        self.interrupted = interrupted
        self.tainted = tainted
    }
}

// MARK: Approvals

/// A live approval from the `approval` event.
public struct ApprovalRequest: Sendable, Equatable, Identifiable {
    public var id: String
    public var tool: String
    public var summary: String
    /// The effect label from the core: `write`, `exec`, `external`, `sensitive-write`, ...
    public var risk: String
    public var detail: String
    public var alwaysRule: String?
    public var canAlways: Bool
    public var canSession: Bool
    /// Not in the §4 event table: whether the conversation had read outside content, and what.
    /// The client derives them (see DESIGN-CHOICES.md); the stub sets them directly.
    public var tainted: Bool
    public var outsideSource: String?
    /// One line of why, from the run ("Heron wants to save the reply draft for Northwind.").
    public var reason: String?
    public var sessionID: String?

    /// Sends, deletes, spends and other actions that can't be undone.
    public var isHighRisk: Bool {
        ["sensitive", "sensitive-write", "DANGEROUS", "refused", "forbidden", "external", "egress"].contains(risk)
    }

    public init(id: String, tool: String, summary: String, risk: String, detail: String, alwaysRule: String?, canAlways: Bool, canSession: Bool, tainted: Bool, outsideSource: String?, reason: String?, sessionID: String?) {
        self.id = id
        self.tool = tool
        self.summary = summary
        self.risk = risk
        self.detail = detail
        self.alwaysRule = alwaysRule
        self.canAlways = canAlways
        self.canSession = canSession
        self.tainted = tainted
        self.outsideSource = outsideSource
        self.reason = reason
        self.sessionID = sessionID
    }
}

public enum ApprovalDecision: String, Sendable, Equatable {
    case once, always, session, deny
}

/// A parked approval (`GET /api/approvals`, src/store.rs `ParkedApproval`).
public struct ParkedApproval: Sendable, Equatable, Identifiable {
    public var id: Int
    public var timestamp: Date
    public var sessionID: String
    public var tool: String
    public var summary: String
    public var detail: String
    public var risk: String
    public var tainted: Bool
    /// What Heron was doing at the time (the job or suggestion title).
    public var origin: String
    public var status: String

    public var isHighRisk: Bool {
        ["sensitive", "sensitive-write", "DANGEROUS", "refused", "forbidden", "external", "egress"].contains(risk)
    }

    public init(id: Int, timestamp: Date, sessionID: String, tool: String, summary: String, detail: String, risk: String, tainted: Bool, origin: String, status: String) {
        self.id = id
        self.timestamp = timestamp
        self.sessionID = sessionID
        self.tool = tool
        self.summary = summary
        self.detail = detail
        self.risk = risk
        self.tainted = tainted
        self.origin = origin
        self.status = status
    }
}

public enum ParkedAction: String, Sendable, Equatable { case approve, dismiss }

// MARK: Today, inbox, memory, tasks, schedules, activity

public struct CalendarEvent: Sendable, Equatable, Identifiable {
    public var id: String
    public var title: String
    /// Nil when the core's text for the time isn't a date ("tomorrow 10:00"); then `whenText` is what to show.
    public var start: Date?
    public var place: String?
    /// The time as the core served it (`GET /api/today` sends `"Title — When: tomorrow 10:00"` lines, not dates).
    public var whenText: String

    public init(id: String, title: String, start: Date?, place: String?, whenText: String = "") {
        self.id = id
        self.title = title
        self.start = start
        self.place = place
        self.whenText = whenText
    }
}

public struct DueTask: Sendable, Equatable, Identifiable {
    public var id: Int
    public var title: String
    public var due: String?
    public var overdue: Bool

    public init(id: Int, title: String, due: String?, overdue: Bool) {
        self.id = id
        self.title = title
        self.due = due
        self.overdue = overdue
    }
}

/// `GET /api/today`.
public struct TodaySummary: Sendable, Equatable {
    public var events: [CalendarEvent]
    public var dueTasks: [DueTask]
    public var openTasks: Int
    public var heronTasks: Int
    public var waiting: Int
    public var unconfirmed: Int
    public var newMail: Int
    public var unread: Int
    public var paused: Bool

    public init(events: [CalendarEvent], dueTasks: [DueTask], openTasks: Int, heronTasks: Int, waiting: Int, unconfirmed: Int, newMail: Int, unread: Int, paused: Bool) {
        self.events = events
        self.dueTasks = dueTasks
        self.openTasks = openTasks
        self.heronTasks = heronTasks
        self.waiting = waiting
        self.unconfirmed = unconfirmed
        self.newMail = newMail
        self.unread = unread
        self.paused = paused
    }
}

public struct InboxItem: Sendable, Equatable, Identifiable {
    public var id: Int
    public var timestamp: Date
    public var title: String
    public var body: String
    public var read: Bool

    public init(id: Int, timestamp: Date, title: String, body: String, read: Bool) {
        self.id = id
        self.timestamp = timestamp
        self.title = title
        self.body = body
        self.read = read
    }
}

public struct Suggestion: Sendable, Equatable, Identifiable {
    public var id: Int
    public var timestamp: Date
    public var title: String
    public var why: String
    public var prompt: String
    /// new | prepared | done | failed | accepted | dismissed
    public var status: String

    public init(id: Int, timestamp: Date, title: String, why: String, prompt: String, status: String) {
        self.id = id
        self.timestamp = timestamp
        self.title = title
        self.why = why
        self.prompt = prompt
        self.status = status
    }
}

public enum SuggestionAnswer: String, Sendable, Equatable { case accept, dismiss }

public struct Memory: Sendable, Equatable, Identifiable {
    public var id: Int
    /// fact, person, project, preference...
    public var kind: String
    public var content: String
    /// user, agent, auto, onboarding, email, calendar, documents, observed, computer.
    public var source: String
    public var confirmed: Bool
    public var uses: Int
    public var createdAt: Date

    public init(id: Int, kind: String, content: String, source: String, confirmed: Bool, uses: Int, createdAt: Date) {
        self.id = id
        self.kind = kind
        self.content = content
        self.source = source
        self.confirmed = confirmed
        self.uses = uses
        self.createdAt = createdAt
    }
}

public struct TaskItem: Sendable, Equatable, Identifiable {
    public var id: Int
    public var title: String
    public var notes: String
    /// open | done
    public var status: String
    public var due: String?
    /// `user` or `heron`.
    public var owner: String
    /// "14 of 22 receipts" for work Heron is doing.
    public var progress: String?
    public var completedAt: Date?

    public init(id: Int, title: String, notes: String, status: String, due: String?, owner: String, progress: String?, completedAt: Date?) {
        self.id = id
        self.title = title
        self.notes = notes
        self.status = status
        self.due = due
        self.owner = owner
        self.progress = progress
        self.completedAt = completedAt
    }
}

public struct ScheduleItem: Sendable, Equatable, Identifiable {
    public var id: Int
    public var name: String
    public var prompt: String
    /// "Every day at 07:30" (from `describe_trigger`) or the cron expression.
    public var trigger: String
    public var enabled: Bool
    public var nextRun: Date?
    public var lastRun: Date?
    public var lastResult: String?
    public var tokens: Int
    /// Why a held schedule is held, in one line.
    public var heldReason: String?
    /// "May only read and look things up".
    public var permissionsNote: String

    public init(id: Int, name: String, prompt: String, trigger: String, enabled: Bool, nextRun: Date?, lastRun: Date?, lastResult: String?, tokens: Int, heldReason: String?, permissionsNote: String) {
        self.id = id
        self.name = name
        self.prompt = prompt
        self.trigger = trigger
        self.enabled = enabled
        self.nextRun = nextRun
        self.lastRun = lastRun
        self.lastResult = lastResult
        self.tokens = tokens
        self.heldReason = heldReason
        self.permissionsNote = permissionsNote
    }
}

public struct AuditEntry: Sendable, Equatable, Identifiable {
    public var id: Int
    public var timestamp: Date
    public var sessionID: String
    public var tool: String
    public var args: String
    /// allowed, approved, approved-always, approved-for-conversation, refused...
    public var decision: String
    public var result: String
    public var provider: String
    public var cliVersion: String

    public var isRefused: Bool { decision.contains("refus") || decision.contains("denied") }
    public var isChange: Bool { !["read_file", "list_dir", "search_files", "find_files", "web_search"].contains(tool) }

    public init(id: Int, timestamp: Date, sessionID: String, tool: String, args: String, decision: String, result: String, provider: String, cliVersion: String) {
        self.id = id
        self.timestamp = timestamp
        self.sessionID = sessionID
        self.tool = tool
        self.args = args
        self.decision = decision
        self.result = result
        self.provider = provider
        self.cliVersion = cliVersion
    }
}

public struct AuditLog: Sendable, Equatable {
    public var entries: [AuditEntry]
    public var intact: Bool

    public init(entries: [AuditEntry], intact: Bool) {
        self.entries = entries
        self.intact = intact
    }
}

// MARK: Persona, risk notice, providers, settings

public struct Persona: Sendable, Equatable {
    public var name: String
    public var text: String
    public var isDefault: Bool
    public var maxCharacters: Int

    public init(name: String, text: String, isDefault: Bool, maxCharacters: Int) {
        self.name = name
        self.text = text
        self.isDefault = isDefault
        self.maxCharacters = maxCharacters
    }
}

/// `GET /api/risk-notice`. The text is shown exactly as served.
public struct RiskNotice: Sendable, Equatable {
    public var version: Int
    public var text: String
    public var acceptedVersion: Int?

    public init(version: Int, text: String, acceptedVersion: Int?) {
        self.version = version
        self.text = text
        self.acceptedVersion = acceptedVersion
    }
}

/// `GET /api/providers`, one row per registry entry.
public struct ProviderStatus: Sendable, Equatable, Identifiable {
    public var id: String
    public var displayName: String
    public var installed: Bool
    public var version: String?
    /// "2.0 to 2.1", when the registry has a tested range.
    public var testedRange: String?
    public var inRange: Bool
    public var loggedIn: Bool
    public var experimental: Bool
    public var versionAccepted: Bool
    public var loginCommand: String
    public var authKind: String
    public var isActive: Bool

    public init(id: String, displayName: String, installed: Bool, version: String?, testedRange: String?, inRange: Bool, loggedIn: Bool, experimental: Bool, versionAccepted: Bool, loginCommand: String, authKind: String, isActive: Bool) {
        self.id = id
        self.displayName = displayName
        self.installed = installed
        self.version = version
        self.testedRange = testedRange
        self.inRange = inRange
        self.loggedIn = loggedIn
        self.experimental = experimental
        self.versionAccepted = versionAccepted
        self.loginCommand = loginCommand
        self.authKind = authKind
        self.isActive = isActive
    }
}

public struct Grant: Sendable, Equatable, Identifiable {
    public var rule: String
    /// `read` for `read:` rules, `tool` for the rest.
    public var kind: String
    public var id: String { rule }

    public init(rule: String, kind: String) {
        self.rule = rule
        self.kind = kind
    }
}

/// One "never" rule from `GET /api/nevers`: something Heron always refuses, whatever the permission mode.
public struct NeverRule: Sendable, Equatable, Identifiable {
    /// The exact rule, such as `web_fetch:*` or `shell:*sudo*`. `DELETE /api/nevers` takes it back verbatim.
    public var rule: String
    /// The tool the rule is about (`web_fetch`).
    public var tool: String
    /// Whether `config.toml` lists the rule. False means only Heron's own store holds it.
    public var inFile: Bool
    public var id: String { rule }

    public init(rule: String, tool: String, inFile: Bool) {
        self.rule = rule
        self.tool = tool
        self.inFile = inFile
    }
}

public struct RuleNotice: Sendable, Equatable, Identifiable {
    public var rule: String
    public var notice: String
    public var id: String { rule }

    public init(rule: String, notice: String) {
        self.rule = rule
        self.notice = notice
    }
}

public struct ConnectionInfo: Sendable, Equatable, Identifiable {
    public var id: String
    public var name: String
    public var connected: Bool
    public var detail: String

    public init(id: String, name: String, connected: Bool, detail: String) {
        self.id = id
        self.name = name
        self.connected = connected
        self.detail = detail
    }
}

/// `GET /api/settings` (the parts the app shows).
public struct CoreSettings: Sendable, Equatable {
    public var workspace: String
    public var mode: PermissionMode
    public var ruleNotices: [RuleNotice]
    public var allowSubscriptionUnattended: Bool
    public var connections: [ConnectionInfo]
    public var awareness: Bool
    public var phoneConnected: Bool

    public init(workspace: String, mode: PermissionMode, ruleNotices: [RuleNotice], allowSubscriptionUnattended: Bool, connections: [ConnectionInfo], awareness: Bool, phoneConnected: Bool) {
        self.workspace = workspace
        self.mode = mode
        self.ruleNotices = ruleNotices
        self.allowSubscriptionUnattended = allowSubscriptionUnattended
        self.connections = connections
        self.awareness = awareness
        self.phoneConnected = phoneConnected
    }
}

public struct UsageDay: Sendable, Equatable, Identifiable {
    public var day: String
    public var model: String
    public var promptTokens: Int
    public var completionTokens: Int
    public var id: String { day + model }

    public init(day: String, model: String, promptTokens: Int, completionTokens: Int) {
        self.day = day
        self.model = model
        self.promptTokens = promptTokens
        self.completionTokens = completionTokens
    }
}

public struct UsageReport: Sendable, Equatable {
    public var days: [UsageDay]
    public var monthTokens: Int

    public init(days: [UsageDay], monthTokens: Int) {
        self.days = days
        self.monthTokens = monthTokens
    }
}
