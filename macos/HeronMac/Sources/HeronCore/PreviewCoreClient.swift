import Foundation

/// A `CoreClientProtocol` backed by in-memory sample data. It drives the app shell in U4a, and stays for
/// previews and tests after U4b adds the live client. Replies are scripted: ask for a "draft" or a "file"
/// and a tool call waits for approval; say "fail" and the run ends with an `error`.
public final class PreviewCoreClient: CoreClientProtocol, @unchecked Sendable {
    public let endpoint: CoreEndpoint

    private let lock = NSLock()
    private var data: PreviewData
    private let now: Date
    private let pace: Duration

    // Event bus: a ring of everything published, and the live subscribers.
    private var seq = 0
    private var ring: [CoreEvent] = []
    private var subscribers: [UUID: AsyncStream<CoreEvent>.Continuation] = [:]
    private var waiters: [String: (sid: String, continuation: CheckedContinuation<ApprovalDecision, Never>)] = [:]
    private var activeRuns: Set<String> = []

    /// - Parameters:
    ///   - pace: the pause between scripted events. Zero in tests, a little in the running app.
    public init(endpoint: CoreEndpoint = CoreEndpoint(port: 0, token: "preview", backgroundOwner: true, version: "preview"),
                now: Date = Date(), pace: Duration = .zero) {
        self.endpoint = endpoint
        self.now = now
        self.pace = pace
        self.data = PreviewData.sample(now: now)
    }

    private func with<T>(_ body: (inout PreviewData) throws -> T) rethrows -> T {
        lock.lock(); defer { lock.unlock() }
        return try body(&data)
    }

    // MARK: Test hooks

    /// The risk register changed while a screen was open.
    public func simulateRiskNoticeChange() {
        with { $0.riskNotice.version += 1; $0.riskNotice.acceptedVersion = nil }
    }

    public func setSandbox(_ kind: String) { with { $0.sandbox = kind } }

    // MARK: State and conversations

    public func state() async throws -> CoreState {
        with { d in
            CoreState(name: d.persona.name, version: "1.0.0-preview", model: d.activeModel, mode: d.mode,
                      workspace: d.workspace, paused: d.paused, waiting: d.parked.filter { $0.status == "pending" }.count,
                      unread: d.inbox.filter { !$0.read }.count, memories: d.memories.count, onboarded: d.onboarded,
                      riskNoticeAccepted: d.riskNotice.acceptedVersion == d.riskNotice.version,
                      sandbox: d.sandbox, hasKey: true)
        }
    }

    public func sessions() async throws -> [SessionSummary] {
        with { d in d.sessions.values.sorted { $0.summary.updatedAt > $1.summary.updatedAt }.map(\.summary) }
    }

    public func session(id: String) async throws -> SessionDetail {
        let busy = lock.withLock { activeRuns.contains(id) }
        return try with { d in
            guard let s = d.sessions[id] else { throw CoreError.notFound }
            var detail = s.detail
            detail.busy = busy
            return detail
        }
    }

    public func deleteSession(id: String) async throws {
        if lock.withLock({ activeRuns.contains(id) }) { throw CoreError.failed("Stop the running reply before deleting this conversation.") }
        with { _ = $0.sessions.removeValue(forKey: id) }
    }

    public func sendMessage(sessionID: String?, message: String, title: String?) async throws -> ChatStart {
        let accepted = with { $0.riskNotice.acceptedVersion == $0.riskNotice.version }
        guard accepted else { throw CoreError.riskNoticeNotAccepted }
        let sid = sessionID ?? "s-\(UUID().uuidString.prefix(8).lowercased())"
        let runID = UUID().uuidString.lowercased()
        let busy = lock.withLock { () -> Bool in
            if activeRuns.contains(sid) { return true }
            activeRuns.insert(sid)
            return false
        }
        if busy { throw CoreError.sessionBusy }
        with { d in
            var s = d.sessions[sid] ?? PreviewSession(id: sid, title: title ?? Self.title(from: message), now: now)
            s.append(role: "user", content: message, now: now)
            d.sessions[sid] = s
        }
        publish(sid, runID, .session(id: sid, runID: runID))
        let script = Self.script(for: message)
        Task { [self] in await run(script: script, sid: sid, runID: runID) }
        return ChatStart(sessionID: sid, runID: runID)
    }

    public func cancel(sessionID: String) async throws -> Bool {
        let wasActive = lock.withLock { activeRuns.contains(sessionID) }
        guard wasActive else { return false }
        // Release a run that waits for an approval, then end it.
        let ids = lock.withLock { Array(waiters.keys) }
        for id in ids where lock.withLock({ waiters[id]?.sid == sessionID }) { resolveWaiter(id, .deny, event: "cancelled") }
        _ = lock.withLock { activeRuns.remove(sessionID) }
        publish(sessionID, nil, .cancelled)
        publish(sessionID, nil, .runEnd)
        return true
    }

    public func events(since: Int?) -> AsyncStream<CoreEvent> {
        AsyncStream { continuation in
            let token = UUID()
            lock.lock()
            if let since { ring.filter { $0.seq > since }.forEach { continuation.yield($0) } }
            subscribers[token] = continuation
            lock.unlock()
            continuation.onTermination = { [weak self] _ in
                guard let self else { return }
                self.lock.lock(); self.subscribers[token] = nil; self.lock.unlock()
            }
        }
    }

    // MARK: Approvals

    public func approve(id: String, decision: ApprovalDecision) async throws {
        resolveWaiter(id, decision, event: decision.rawValue)
    }

    public func approvals() async throws -> [ParkedApproval] {
        with { $0.parked.filter { $0.status == "pending" } }
    }

    public func answerParked(id: Int, action: ParkedAction) async throws {
        try with { d in
            guard let i = d.parked.firstIndex(where: { $0.id == id }) else { throw CoreError.notFound }
            d.parked[i].status = action == .approve ? "approved" : "dismissed"
        }
    }

    // MARK: Mode, pause, today, inbox

    public func setMode(_ mode: PermissionMode) async throws { with { $0.mode = mode } }
    public func setPaused(_ paused: Bool) async throws { with { $0.paused = paused } }

    public func today() async throws -> TodaySummary {
        let today = PreviewData.isoDay(now, offset: 0)
        return with { d in
            TodaySummary(events: d.events, dueTasks: d.tasks.filter { $0.status == "open" && $0.owner == "user" && ($0.due ?? "9999") <= today }
                .map { DueTask(id: $0.id, title: $0.title, due: $0.due, overdue: ($0.due ?? "") < today) },
                         openTasks: d.tasks.filter { $0.status == "open" && $0.owner == "user" }.count,
                         heronTasks: d.tasks.filter { $0.status == "open" && $0.owner == "heron" }.count,
                         waiting: d.parked.filter { $0.status == "pending" }.count, unconfirmed: d.memories.filter { !$0.confirmed }.count,
                         newMail: 4, unread: d.inbox.filter { !$0.read }.count, paused: d.paused)
        }
    }

    public func inbox() async throws -> [InboxItem] { with { $0.inbox } }
    public func markInboxRead() async throws { with { d in for i in d.inbox.indices { d.inbox[i].read = true } } }
    public func suggestions() async throws -> [Suggestion] { with { $0.suggestions } }

    public func answerSuggestion(id: Int, answer: SuggestionAnswer) async throws {
        try with { d in
            guard let i = d.suggestions.firstIndex(where: { $0.id == id }) else { throw CoreError.notFound }
            d.suggestions[i].status = answer == .accept ? "accepted" : "dismissed"
        }
    }

    // MARK: Memory, tasks, schedules, audit

    public func memories(query: String?) async throws -> [Memory] {
        with { d in
            guard let q = query?.trimmingCharacters(in: .whitespaces).lowercased(), !q.isEmpty else { return d.memories }
            return d.memories.filter { $0.content.lowercased().contains(q) || $0.kind.lowercased().contains(q) }
        }
    }

    public func addMemory(content: String, kind: String) async throws -> Int {
        with { d in
            let id = (d.memories.map(\.id).max() ?? 0) + 1
            d.memories.append(Memory(id: id, kind: kind, content: content, source: "user", confirmed: true, uses: 0, createdAt: now))
            return id
        }
    }

    public func confirmMemory(id: Int) async throws {
        try with { d in
            guard let i = d.memories.firstIndex(where: { $0.id == id }) else { throw CoreError.notFound }
            d.memories[i].confirmed = true
        }
    }

    public func deleteMemory(id: Int) async throws {
        try with { d in
            guard d.memories.contains(where: { $0.id == id }) else { throw CoreError.notFound }
            d.memories.removeAll { $0.id == id }
        }
    }

    public func tasks() async throws -> [TaskItem] { with { $0.tasks } }

    public func addTask(title: String, due: String?) async throws -> Int {
        with { d in
            let id = (d.tasks.map(\.id).max() ?? 0) + 1
            d.tasks.append(TaskItem(id: id, title: title, notes: "", status: "open", due: due, owner: "user", progress: nil, completedAt: nil))
            return id
        }
    }

    public func setTaskDone(id: Int, done: Bool) async throws {
        try with { d in
            guard let i = d.tasks.firstIndex(where: { $0.id == id }) else { throw CoreError.notFound }
            d.tasks[i].status = done ? "done" : "open"
            d.tasks[i].completedAt = done ? now : nil
        }
    }

    public func deleteTask(id: Int) async throws {
        try with { d in
            guard d.tasks.contains(where: { $0.id == id }) else { throw CoreError.notFound }
            d.tasks.removeAll { $0.id == id }
        }
    }

    public func schedules() async throws -> [ScheduleItem] { with { $0.schedules } }

    public func runSchedule(id: Int) async throws {
        try with { d in
            guard let i = d.schedules.firstIndex(where: { $0.id == id }) else { throw CoreError.notFound }
            d.schedules[i].lastRun = now
            d.schedules[i].lastResult = "Ran just now."
        }
    }

    public func setScheduleEnabled(id: Int, enabled: Bool) async throws {
        try with { d in
            guard let i = d.schedules.firstIndex(where: { $0.id == id }) else { throw CoreError.notFound }
            d.schedules[i].enabled = enabled
            d.schedules[i].heldReason = enabled ? nil : d.schedules[i].heldReason
        }
    }

    public func audit() async throws -> AuditLog { with { AuditLog(entries: $0.audit, intact: true) } }

    // MARK: Persona and first run

    public func persona() async throws -> Persona { with { $0.persona } }

    public func setPersona(name: String?, text: String?) async throws -> Persona {
        with { d in
            if let name = name?.trimmingCharacters(in: .whitespaces), !name.isEmpty, name.count <= 40 { d.persona.name = name }
            if let text { d.persona.text = text; d.persona.isDefault = false }
            return d.persona
        }
    }

    public func riskNotice() async throws -> RiskNotice { with { $0.riskNotice } }

    public func acceptRiskNotice(version: Int) async throws {
        try with { d in
            guard version == d.riskNotice.version else { throw CoreError.riskNoticeOutdated }
            d.riskNotice.acceptedVersion = version
        }
    }

    public func providers() async throws -> [ProviderStatus] { with { $0.providers } }

    public func acceptProviderVersion(id: String, version: String) async throws {
        try with { d in
            guard let i = d.providers.firstIndex(where: { $0.id == id }) else { throw CoreError.notFound }
            d.providers[i].versionAccepted = true
        }
    }

    public func selectProvider(id: String) async throws {
        try with { d in
            guard let p = d.providers.first(where: { $0.id == id }) else { throw CoreError.notFound }
            for i in d.providers.indices { d.providers[i].isActive = d.providers[i].id == id }
            d.activeModel = p.displayName
        }
    }

    public func setOnboarded() async throws { with { $0.onboarded = true } }

    // MARK: Settings

    public func settings() async throws -> CoreSettings {
        with { d in
            CoreSettings(workspace: d.workspace, mode: d.mode, ruleNotices: d.ruleNotices,
                         allowSubscriptionUnattended: d.allowSubscriptionUnattended, connections: d.connections,
                         awareness: d.awareness, phoneConnected: false)
        }
    }

    public func setUnattendedSubscription(_ allowed: Bool) async throws { with { $0.allowSubscriptionUnattended = allowed } }
    public func setAwareness(_ enabled: Bool) async throws { with { $0.awareness = enabled } }
    public func grants() async throws -> [Grant] { with { $0.grants } }

    public func revokeGrant(rule: String) async throws {
        try with { d in
            guard d.grants.contains(where: { $0.rule == rule }) else { throw CoreError.notFound }
            d.grants.removeAll { $0.rule == rule }
        }
    }

    public func usage() async throws -> UsageReport { with { $0.usage } }

    // MARK: Scripted runs

    private enum Script { case plain, approval(needsSession: Bool), failure }

    private static func script(for message: String) -> Script {
        let m = message.lowercased()
        if m.contains("fail") { return .failure }
        if m.contains("draft") || m.contains("file") || m.contains("tidy") { return .approval(needsSession: true) }
        return .plain
    }

    private static func title(from message: String) -> String {
        let t = message.trimmingCharacters(in: .whitespacesAndNewlines)
        return t.count > 40 ? String(t.prefix(40)) + "…" : t
    }

    private func pause() async { if pace != .zero { try? await Task.sleep(for: pace) } }

    private func run(script: Script, sid: String, runID: String) async {
        func alive() -> Bool { lock.withLock { activeRuns.contains(sid) } }
        await pause()
        guard alive() else { return }
        publish(sid, runID, .toolStart(id: "t1", name: "upcoming_events", summary: "Reading your calendar…", risk: "safe"))
        await pause()
        guard alive() else { return }
        publish(sid, runID, .toolEnd(id: "t1", name: "upcoming_events", ok: true, preview: "3 events today"))

        switch script {
        case .failure:
            publish(sid, runID, .error(message: "Claude Code version 3.0.1 hasn't been tested with Heron.", code: "cli_version_untested",
                                       provider: "claude-cli", version: "3.0.1"))
            finish(sid: sid, runID: runID, text: nil)
            return
        case .approval:
            let request = ApprovalRequest(
                id: "a-\(UUID().uuidString.prefix(8).lowercased())", tool: "write_file",
                summary: "Create a file in your working folder", risk: "write",
                detail: "~/Work/drafts/northwind-reply.md", alwaysRule: "write_file:~/Work/drafts/**",
                canAlways: true, canSession: true, tainted: false, outsideSource: nil,
                reason: "Heron wants to save the reply draft for Northwind.", sessionID: sid)
            publish(sid, runID, .approval(request))
            let decision = await withCheckedContinuation { (c: CheckedContinuation<ApprovalDecision, Never>) in
                lock.withLock { waiters[request.id] = (sid, c) }
            }
            guard alive() else { return }
            if decision == .deny {
                publish(sid, runID, .text("Understood. I didn't save the draft."))
                finish(sid: sid, runID: runID, text: "Understood. I didn't save the draft.")
                return
            }
            publish(sid, runID, .toolStart(id: "t2", name: "write_file", summary: "Saving the draft", risk: "write"))
            await pause()
            publish(sid, runID, .toolEnd(id: "t2", name: "write_file", ok: true, preview: "Saved northwind-reply.md"))
            fallthroughReply(sid: sid, runID: runID, text: "I saved the draft in ~/Work/drafts. It thanks Maria and offers Thursday.")
        case .plain:
            fallthroughReply(sid: sid, runID: runID, text: "You have three events today. The Northwind call at 14:00 matters most; the other two can move.")
        }
    }

    private func fallthroughReply(sid: String, runID: String, text: String) {
        publish(sid, runID, .text(text))
        finish(sid: sid, runID: runID, text: text)
    }

    private func finish(sid: String, runID: String, text: String?) {
        lock.withLock {
            if let text { data.sessions[sid]?.append(role: "assistant", content: text, now: now) }
            activeRuns.remove(sid)
        }
        if text != nil { publish(sid, runID, .done(text: text ?? "", title: nil)) }
        publish(sid, runID, .runEnd)
    }

    private func resolveWaiter(_ id: String, _ decision: ApprovalDecision, event: String) {
        let waiter = lock.withLock { waiters.removeValue(forKey: id) }
        guard let waiter else { return }
        publish(waiter.sid, nil, .approvalResolved(id: id, decision: event))
        waiter.continuation.resume(returning: decision)
    }

    private func publish(_ sid: String?, _ runID: String?, _ kind: CoreEvent.Kind) {
        lock.lock()
        seq += 1
        let event = CoreEvent(seq: seq, sessionID: sid, runID: runID, kind: kind)
        ring.append(event)
        if ring.count > 10_000 { ring.removeFirst(ring.count - 10_000) }
        let targets = Array(subscribers.values)
        lock.unlock()
        targets.forEach { $0.yield(event) }
    }
}
