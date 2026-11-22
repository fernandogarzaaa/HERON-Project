import Foundation

/// Decoding of the core's wire formats (heron-v1.md §3 and §4, `src/server.rs`, `src/store.rs`,
/// `src/events.rs`) into the plain models the views read.
///
/// The wire types are private `Decodable` DTOs; the app's models stay plain structs so previews and
/// snapshots keep building them directly. Required fields throw `DecodingError`: a field the core stops
/// sending fails loudly instead of turning into a default. Only fields the core omits by design are optional.
public enum CoreWire {
    // MARK: Decoder

    static func decoder() -> JSONDecoder {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        return d
    }

    private static func decode<T: Decodable>(_ type: T.Type, _ data: Data) throws -> T {
        try decoder().decode(type, from: data)
    }

    private static func date(_ unix: Int) -> Date { Date(timeIntervalSince1970: TimeInterval(unix)) }

    // MARK: Endpoint line

    /// `{"port":…,"token":"…","background_owner":true,"version":"0.3.1"}`, the one line `serve --print-endpoint` prints.
    public static func endpoint(line: String) throws -> CoreEndpoint {
        struct DTO: Decodable { var port: Int; var token: String; var backgroundOwner: Bool; var version: String }
        let dto = try decode(DTO.self, Data(line.utf8))
        return CoreEndpoint(port: dto.port, token: dto.token, backgroundOwner: dto.backgroundOwner, version: dto.version)
    }

    // MARK: State

    public static func state(_ data: Data) throws -> CoreState {
        struct DTO: Decodable {
            var name: String; var version: String; var model: String; var mode: String; var workspace: String
            var paused: Bool; var waiting: Int; var unread: Int; var memories: Int; var onboarded: Bool
            var riskNoticeAccepted: Bool; var sandbox: String; var hasKey: Bool
        }
        let d = try decode(DTO.self, data)
        return CoreState(name: d.name, version: d.version, model: d.model, mode: try mode(d.mode), workspace: d.workspace,
                         paused: d.paused, waiting: d.waiting, unread: d.unread, memories: d.memories, onboarded: d.onboarded,
                         riskNoticeAccepted: d.riskNoticeAccepted, sandbox: d.sandbox, hasKey: d.hasKey)
    }

    static func mode(_ name: String) throws -> PermissionMode {
        guard let m = PermissionMode(rawValue: name) else {
            throw DecodingError.dataCorrupted(.init(codingPath: [], debugDescription: "unknown permission mode \(name)"))
        }
        return m
    }

    // MARK: Sessions

    public static func sessions(_ data: Data) throws -> [SessionSummary] {
        struct DTO: Decodable { var id: String; var title: String; var updatedAt: Int; var messages: Int }
        return try decode([DTO].self, data).map { SessionSummary(id: $0.id, title: $0.title, updatedAt: date($0.updatedAt), messages: $0.messages) }
    }

    /// `GET /api/sessions/{id}`. The core serves the model's raw transcript (user, assistant with `tool_calls`,
    /// `tool` results); this folds it into what the chat shows: tool results become the steps of the assistant
    /// message that called them, and an assistant turn split by tool calls reads as one message.
    public static func session(_ data: Data) throws -> SessionDetail {
        struct Call: Decodable { struct Fn: Decodable { var name: String; var arguments: String? }; var id: String; var function: Fn }
        struct Raw: Decodable { var role: String; var content: String?; var toolCalls: [Call]?; var toolCallId: String? }
        struct DTO: Decodable { var id: String; var title: String; var messages: [Raw]; var busy: Bool; var interrupted: Bool; var tainted: Bool? }
        let d = try decode(DTO.self, data)

        var out: [SessionMessage] = []
        // Where each call's step lives, to attach its result: (message index in `out`, step index).
        var stepOf: [String: (Int, Int)] = [:]
        var lastWasAssistant = false
        for (index, raw) in d.messages.enumerated() {
            switch raw.role {
            case "user":
                out.append(SessionMessage(id: index, role: "user", content: raw.content ?? "", toolCalls: []))
                lastWasAssistant = false
            case "assistant":
                let steps = (raw.toolCalls ?? []).map {
                    ToolCallInfo(id: $0.id, name: $0.function.name, summary: stepSentence(name: $0.function.name, arguments: $0.function.arguments),
                                 ok: nil, durationMs: nil, untrusted: false)
                }
                let text = raw.content ?? ""
                if lastWasAssistant, var previous = out.popLast() {
                    if !text.isEmpty { previous.content += previous.content.isEmpty ? text : "\n\n" + text }
                    previous.toolCalls += steps
                    out.append(previous)
                } else {
                    out.append(SessionMessage(id: index, role: "assistant", content: text, toolCalls: steps))
                }
                let at = out.count - 1
                for (n, step) in out[at].toolCalls.enumerated() { stepOf[step.id] = (at, n) }
                lastWasAssistant = true
            case "tool":
                if let id = raw.toolCallId, let (m, s) = stepOf[id] {
                    out[m].toolCalls[s].ok = !(raw.content ?? "").hasPrefix("Error")
                }
            default:
                break
            }
        }
        return SessionDetail(id: d.id, title: d.title, messages: out, busy: d.busy, interrupted: d.interrupted, tainted: d.tainted ?? false)
    }

    /// "write file · plan.md": the tool's name in words and its first argument, since the transcript has no summary.
    static func stepSentence(name: String, arguments: String?) -> String {
        let words = name.replacingOccurrences(of: "_", with: " ")
        guard let arguments, let object = try? JSONSerialization.jsonObject(with: Data(arguments.utf8)) as? [String: Any] else { return words }
        for key in ["path", "command", "query", "url", "title", "to", "name"] {
            if let value = object[key] as? String, !value.isEmpty {
                let one = value.split(whereSeparator: \.isNewline).first.map(String.init) ?? value
                return "\(words) · \(one.count > 60 ? String(one.prefix(60)) + "…" : one)"
            }
        }
        return words
    }

    // MARK: Approvals

    /// `GET /api/approvals/live`: each item is an `approval` event with its `session_id`, decoded the same way.
    public static func liveApprovals(_ data: Data) throws -> [ApprovalRequest] {
        guard let items = try JSONSerialization.jsonObject(with: data) as? [Any] else {
            throw DecodingError.dataCorrupted(.init(codingPath: [], debugDescription: "live approvals are a JSON array"))
        }
        return try items.map { item in
            let one = try JSONSerialization.data(withJSONObject: item)
            guard case .approval(let request)? = try envelope(one).event?.kind else {
                throw DecodingError.dataCorrupted(.init(codingPath: [], debugDescription: "a live approval is an approval event"))
            }
            return request
        }
    }

    public static func parkedApprovals(_ data: Data) throws -> [ParkedApproval] {
        struct DTO: Decodable {
            var id: Int; var ts: Int; var sessionId: String; var tool: String; var summary: String; var detail: String
            var risk: String; var tainted: Bool; var origin: String; var status: String
        }
        return try decode([DTO].self, data).map {
            ParkedApproval(id: $0.id, timestamp: date($0.ts), sessionID: $0.sessionId, tool: $0.tool, summary: $0.summary, detail: $0.detail,
                           risk: $0.risk, tainted: $0.tainted, origin: $0.origin, status: $0.status)
        }
    }

    // MARK: Today, inbox, suggestions

    public static func today(_ data: Data) throws -> TodaySummary {
        struct Due: Decodable { var id: Int; var title: String; var due: String?; var overdue: Bool }
        struct DTO: Decodable {
            var events: [String]; var dueTasks: [Due]; var openTasks: Int; var heronTasks: Int; var waiting: Int
            var unconfirmed: Int; var newMail: Int; var unread: Int; var paused: Bool
        }
        let d = try decode(DTO.self, data)
        return TodaySummary(events: d.events.enumerated().map { calendarEvent(line: $0.element, index: $0.offset) },
                            dueTasks: d.dueTasks.map { DueTask(id: $0.id, title: $0.title, due: $0.due, overdue: $0.overdue) },
                            openTasks: d.openTasks, heronTasks: d.heronTasks, waiting: d.waiting, unconfirmed: d.unconfirmed,
                            newMail: d.newMail, unread: d.unread, paused: d.paused)
    }

    /// The core serves a calendar line as text, `"Title — When: tomorrow 10:00"` (`sources::upcoming`), not as
    /// fields. The title is everything before the last " — "; the rest is shown as served, and becomes a date
    /// only when it is an ISO 8601 time (Google and Microsoft calendars).
    static func calendarEvent(line: String, index: Int) -> CalendarEvent {
        var text = line
        for prefix in ["[Google Calendar] ", "[Microsoft Calendar] "] where text.hasPrefix(prefix) { text.removeFirst(prefix.count) }
        let title: String
        let when: String
        if let range = text.range(of: " — ", options: .backwards) {
            title = String(text[..<range.lowerBound])
            when = String(text[range.upperBound...])
        } else {
            title = text
            when = ""
        }
        return CalendarEvent(id: "event-\(index)", title: title, start: parseISO8601(when), place: nil, whenText: when)
    }

    static func parseISO8601(_ text: String) -> Date? {
        let plain = ISO8601DateFormatter()
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return plain.date(from: text) ?? fractional.date(from: text)
    }

    public static func inbox(_ data: Data) throws -> [InboxItem] {
        struct DTO: Decodable { var id: Int; var ts: Int; var title: String; var body: String; var read: Bool }
        return try decode([DTO].self, data).map { InboxItem(id: $0.id, timestamp: date($0.ts), title: $0.title, body: $0.body, read: $0.read) }
    }

    public static func suggestions(_ data: Data) throws -> [Suggestion] {
        struct DTO: Decodable { var id: Int; var ts: Int; var title: String; var why: String; var prompt: String; var status: String }
        return try decode([DTO].self, data).map { Suggestion(id: $0.id, timestamp: date($0.ts), title: $0.title, why: $0.why, prompt: $0.prompt, status: $0.status) }
    }

    // MARK: Memory, tasks, schedules, audit

    public static func memories(_ data: Data) throws -> [Memory] {
        struct DTO: Decodable { var id: Int; var kind: String; var content: String; var source: String; var confirmed: Bool; var uses: Int; var createdAt: Int }
        return try decode([DTO].self, data).map {
            Memory(id: $0.id, kind: $0.kind, content: $0.content, source: $0.source, confirmed: $0.confirmed, uses: $0.uses, createdAt: date($0.createdAt))
        }
    }

    /// The core serves neither progress nor a completion time for tasks, so both stay nil (DESIGN-CHOICES.md).
    public static func tasks(_ data: Data) throws -> [TaskItem] {
        struct DTO: Decodable { var id: Int; var title: String; var notes: String; var status: String; var due: String?; var owner: String }
        return try decode([DTO].self, data).map {
            TaskItem(id: $0.id, title: $0.title, notes: $0.notes, status: $0.status, due: $0.due, owner: $0.owner, progress: nil, completedAt: nil)
        }
    }

    public static func schedules(_ data: Data) throws -> [ScheduleItem] {
        struct DTO: Decodable {
            var id: Int; var name: String; var cron: String; var prompt: String; var enabled: Bool
            var nextRun: Int?; var lastRun: Int?; var lastResult: String?; var allow: String; var lastTokens: Int; var trigger: String?
        }
        return try decode([DTO].self, data).map {
            ScheduleItem(id: $0.id, name: $0.name, prompt: $0.prompt, trigger: triggerText(trigger: $0.trigger, cron: $0.cron, nextRun: $0.nextRun),
                         enabled: $0.enabled, nextRun: $0.nextRun.map(date), lastRun: $0.lastRun.map(date), lastResult: $0.lastResult,
                         tokens: $0.lastTokens, heldReason: nil, permissionsNote: permissionsNote(allow: $0.allow))
        }
    }

    /// `describe_trigger` text for `@…` triggers; otherwise the cron expression; a one-shot job has neither.
    static func triggerText(trigger: String?, cron: String, nextRun: Int?) -> String {
        if let trigger, !trigger.isEmpty { return trigger }
        if !cron.isEmpty { return cron }
        return nextRun == nil ? "Once" : "Once, at its start time"
    }

    /// What a job may do while nobody watches, from the rules the user granted it (`grants_for` in the core).
    static func permissionsNote(allow: String) -> String {
        let rules = allow.split(whereSeparator: \.isNewline).map(String.init)
        if rules.contains(where: { $0.hasPrefix("shell:") }) { return "May edit files and run commands" }
        if rules.contains(where: { $0.hasPrefix("write_file:") || $0.hasPrefix("edit_file:") }) { return "May edit files" }
        return "May only read and look things up"
    }

    public static func audit(_ data: Data) throws -> AuditLog {
        struct Entry: Decodable {
            var ts: Int; var sessionId: String; var tool: String; var args: String; var decision: String
            var result: String; var provider: String; var cliVersion: String
        }
        struct DTO: Decodable { var entries: [Entry]; var intact: Bool }
        let d = try decode(DTO.self, data)
        // The core serves no row id; the entry's position stands in (newest first, as served).
        let entries = d.entries.enumerated().map { n, e in
            AuditEntry(id: n, timestamp: date(e.ts), sessionID: e.sessionId, tool: e.tool, args: e.args, decision: e.decision,
                       result: e.result, provider: e.provider, cliVersion: e.cliVersion)
        }
        return AuditLog(entries: entries, intact: d.intact)
    }

    // MARK: Persona, risk notice

    public static func persona(_ data: Data) throws -> Persona {
        struct DTO: Decodable { var name: String; var text: String; var isDefault: Bool; var max: Int }
        let d = try decode(DTO.self, data)
        return Persona(name: d.name, text: d.text, isDefault: d.isDefault, maxCharacters: d.max)
    }

    public static func riskNotice(_ data: Data) throws -> RiskNotice {
        struct DTO: Decodable { var version: Int; var text: String; var acceptedVersion: Int? }
        let d = try decode(DTO.self, data)
        return RiskNotice(version: d.version, text: d.text, acceptedVersion: d.acceptedVersion)
    }

    // MARK: Providers, grants, settings

    struct ProviderInfo {
        var id: String; var name: String; var auth: String; var login: String
    }

    /// `GET /api/providers` joined with `GET /api/settings`: the first has the installed state and versions, the
    /// second the login command, the auth kind and which provider is in use. `settings` may be nil.
    public static func providers(_ data: Data, settings: Data?) throws -> [ProviderStatus] {
        struct DTO: Decodable {
            var id: String; var displayName: String; var installed: Bool; var version: String?; var supported: String?
            var inRange: Bool?; var loggedIn: Bool?; var experimental: Bool; var versionAccepted: Bool
        }
        struct Info: Decodable { var id: String; var auth: String; var login: String }
        struct Settings: Decodable { var provider: String; var providers: [Info] }
        let rows = try decode([DTO].self, data)
        let s = try settings.map { try decode(Settings.self, $0) }
        return rows.map { r in
            let info = s?.providers.first { $0.id == r.id }
            // A CLI whose login can't be checked (`logged_in` null) is treated as signed in: the core reports the
            // sign-in problem itself when a run starts. `in_range` null means no tested range, so not in range.
            return ProviderStatus(id: r.id, displayName: r.displayName, installed: r.installed, version: r.version,
                                  testedRange: r.supported.flatMap(testedRange), inRange: r.inRange ?? false, loggedIn: r.loggedIn ?? true,
                                  experimental: r.experimental, versionAccepted: r.versionAccepted, loginCommand: info?.login ?? "",
                                  authKind: info?.auth ?? "", isActive: s?.provider == r.id)
        }
    }

    /// ">=2.0.0 <3.0.0" → "2.0.0 to below 3.0.0".
    static func testedRange(_ supported: String) -> String? {
        let parts = supported.split(separator: " ")
        guard parts.count == 2, parts[0].hasPrefix(">="), parts[1].hasPrefix("<") else { return supported.isEmpty ? nil : supported }
        return "\(parts[0].dropFirst(2)) to below \(parts[1].dropFirst())"
    }

    public static func grants(_ data: Data) throws -> [Grant] {
        struct DTO: Decodable { var kind: String; var rule: String }
        return try decode([DTO].self, data).map { Grant(rule: $0.rule, kind: $0.kind) }
    }

    /// `GET /api/nevers`: `[{"rule", "tool", "in_file"}]`.
    public static func nevers(_ data: Data) throws -> [NeverRule] {
        struct DTO: Decodable { var rule: String; var tool: String; var inFile: Bool }
        return try decode([DTO].self, data).map { NeverRule(rule: $0.rule, tool: $0.tool, inFile: $0.inFile) }
    }

    /// `GET /api/settings`, plus the three routes the Connections tab needs (`/api/sources`, `/api/awareness`,
    /// `/api/telegram`). Those are optional: when one is unavailable its part stays at the empty default.
    public static func settings(_ data: Data, sources: Data? = nil, awareness: Data? = nil, telegram: Data? = nil) throws -> CoreSettings {
        struct Notice: Decodable { var rule: String; var notice: String }
        struct DTO: Decodable {
            var workspace: String; var mode: String; var ruleNotices: [Notice]; var allowUnattendedSubscription: Bool
        }
        let d = try decode(DTO.self, data)
        struct Awareness: Decodable { var enabled: Bool }
        struct Telegram: Decodable { var paired: Bool }
        return CoreSettings(workspace: d.workspace, mode: try mode(d.mode), ruleNotices: d.ruleNotices.map { RuleNotice(rule: $0.rule, notice: $0.notice) },
                            allowSubscriptionUnattended: d.allowUnattendedSubscription,
                            connections: sources.map(connections) ?? [],
                            awareness: (try? awareness.map { try decode(Awareness.self, $0).enabled }) ?? false,
                            phoneConnected: (try? telegram.map { try decode(Telegram.self, $0).paired }) ?? false)
    }

    /// `GET /api/sources` as rows: one per place Heron can learn from.
    static func connections(_ data: Data) -> [ConnectionInfo] {
        guard let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return [] }
        func row(_ key: String, _ name: String, detail: (([String: Any]) -> String)? = nil) -> ConnectionInfo? {
            guard let entry = object[key] as? [String: Any] else { return nil }
            let connected = entry["connected"] as? Bool ?? false
            let indexed = entry["indexed"] as? Int ?? 0
            let text = connected ? (detail?(entry) ?? "\(indexed) items indexed") : "Not connected"
            return ConnectionInfo(id: key, name: name, connected: connected, detail: text)
        }
        return [
            row("calendar", "Calendar"),
            row("email", "Email") { ($0["address"] as? String) ?? "\($0["indexed"] as? Int ?? 0) messages indexed" },
            row("documents", "Documents") { "\(($0["folders"] as? [String])?.count ?? 0) folders, \($0["indexed"] as? Int ?? 0) files indexed" },
            row("computer", "This Mac"),
        ].compactMap { $0 }
    }

    // MARK: Usage

    /// `GET /api/usage`. The report serves per-day totals and per-model month totals, but the Usage tab shows
    /// one row per (day, model) with tokens in and out; the model rows carry both, so they become the rows.
    public static func usage(_ data: Data) throws -> UsageReport {
        struct Window: Decodable { var used: Int }
        struct Budget: Decodable { var month: Window }
        struct DTO: Decodable { var budget: Budget; var models: [[JSONScalar]] }
        let d = try decode(DTO.self, data)
        let rows = d.models.compactMap { m -> UsageDay? in
            guard m.count >= 3, case .string(let name) = m[0], case .int(let input) = m[1], case .int(let output) = m[2] else { return nil }
            return UsageDay(day: "This month", model: name, promptTokens: input, completionTokens: output)
        }
        return UsageReport(days: rows, monthTokens: d.budget.month.used)
    }

    /// A JSON string or integer, for the core's tuples (`["model", 10, 20, 3]`).
    enum JSONScalar: Decodable {
        case string(String), int(Int), other
        init(from decoder: Decoder) throws {
            let c = try decoder.singleValueContainer()
            if let i = try? c.decode(Int.self) { self = .int(i) } else if let s = try? c.decode(String.self) { self = .string(s) } else { self = .other }
        }
    }

    // MARK: Events

    /// One decoded SSE payload. `event` is nil for a type this client doesn't know: it still advances `seq`,
    /// so a newer core's extra event types never stall a resume.
    public struct Envelope: Sendable, Equatable {
        public var seq: Int?
        public var event: CoreEvent?
        public var isResync: Bool { if case .resync = event?.kind { return true } else { return false } }
    }

    /// Decodes one event envelope: `{seq, session_id, run_id, type, ...}` (or `{type: "resync", oldest}`).
    public static func envelope(_ data: Data) throws -> Envelope {
        guard let o = try JSONSerialization.jsonObject(with: data) as? [String: Any], let type = o["type"] as? String else {
            throw DecodingError.dataCorrupted(.init(codingPath: [], debugDescription: "an event needs a JSON object with a type"))
        }
        func str(_ k: String) throws -> String {
            guard let v = o[k] as? String else { throw DecodingError.keyNotFound(AnyKey(k), .init(codingPath: [], debugDescription: "\(type) event has no \(k)")) }
            return v
        }
        func int(_ k: String) throws -> Int {
            guard let v = o[k] as? Int else { throw DecodingError.keyNotFound(AnyKey(k), .init(codingPath: [], debugDescription: "\(type) event has no \(k)")) }
            return v
        }
        let seq = o["seq"] as? Int
        let sessionID = o["session_id"] as? String
        let runID = o["run_id"] as? String
        func make(_ kind: CoreEvent.Kind) -> Envelope {
            Envelope(seq: seq, event: CoreEvent(seq: seq ?? 0, sessionID: sessionID, runID: runID, kind: kind))
        }
        switch type {
        case "session": return make(.session(id: try str("id"), runID: try str("run_id")))
        case "text": return make(.text(try str("text")))
        case "tool_start": return make(.toolStart(id: try str("id"), name: try str("name"), summary: try str("summary"), risk: try str("risk")))
        case "tool_end": return make(.toolEnd(id: try str("id"), name: try str("name"), ok: o["ok"] as? Bool ?? false, preview: o["preview"] as? String ?? ""))
        case "approval":
            let request = ApprovalRequest(id: try str("id"), tool: try str("tool"), summary: try str("summary"), risk: try str("risk"),
                                          detail: o["detail"] as? String ?? "", alwaysRule: o["always_rule"] as? String,
                                          canAlways: o["can_always"] as? Bool ?? false, canSession: o["can_session"] as? Bool ?? false,
                                          tainted: o["tainted"] as? Bool ?? false, outsideSource: o["outside_source"] as? String,
                                          reason: o["reason"] as? String, sessionID: sessionID)
            return make(.approval(request))
        case "approval_resolved": return make(.approvalResolved(id: try str("id"), decision: try str("decision")))
        case "approval_parked": return make(.approvalParked(id: try str("id"), parkedID: try int("parked_id")))
        case "usage": return make(.usage(prompt: o["prompt"] as? Int ?? 0, completion: o["completion"] as? Int ?? 0))
        case "model": return make(.model(name: try str("name"), why: o["why"] as? String ?? ""))
        case "notice": return make(.notice(try str("text")))
        case "learned": return make(.learned(o["items"] as? [String] ?? []))
        case "done": return make(.done(text: o["text"] as? String ?? "", title: o["title"] as? String))
        case "cancelled": return make(.cancelled)
        case "error":
            return make(.error(message: o["message"] as? String ?? "", code: o["code"] as? String, provider: o["provider"] as? String, version: o["version"] as? String))
        case "run_end": return make(.runEnd)
        case "resync": return Envelope(seq: nil, event: CoreEvent(seq: 0, kind: .resync(oldest: try int("oldest"))))
        default: return Envelope(seq: seq, event: nil)
        }
    }

    /// Decodes a recording in the `events-*.jsonl` format: one envelope per line. Unknown types are skipped.
    public static func events(jsonLines text: String) throws -> [CoreEvent] {
        try text.split(whereSeparator: \.isNewline).compactMap { line in
            try envelope(Data(line.utf8)).event
        }
    }

    private struct AnyKey: CodingKey {
        var stringValue: String
        var intValue: Int? { nil }
        init(_ s: String) { stringValue = s }
        init?(stringValue: String) { self.stringValue = stringValue }
        init?(intValue: Int) { nil }
    }
}
