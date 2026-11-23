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
    /// Approvals a run is waiting on, as `GET /api/approvals/live` returns them.
    private var liveRequests: [String: ApprovalRequest] = [:]
    private var beforeSendReturns: (@Sendable (String) async -> Void)?
    private var sendFailure: CoreError?
    private var cancelFailure: CoreError?
    private var pinned: Set<String> = []
    private var archived: Set<String> = []

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

    /// Runs `body` with the new conversation's id after the run has started but before `sendMessage`
    /// returns, as a real core can deliver events before its `/api/chat` response.
    public func runBeforeSendReturns(_ body: (@Sendable (String) async -> Void)?) { lock.withLock { beforeSendReturns = body } }

    /// The risk register changed while a screen was open.
    public func simulateRiskNoticeChange() {
        with { $0.riskNotice.version += 1; $0.riskNotice.acceptedVersion = nil }
    }

    public func setSandbox(_ kind: String) { with { $0.sandbox = kind } }

    /// What a core restart does to a conversation mid-reply: the run is gone without a `run_end`, its live
    /// approvals are gone, what it saved stays (so `session(id:)` reports it `interrupted`), and the event
    /// stream comes back with `resync`.
    public func simulateCoreRestart() {
        let stranded = lock.withLock { () -> [CheckedContinuation<ApprovalDecision, Never>] in
            for sid in activeRuns { data.sessions[sid]?.detail.interrupted = true }
            activeRuns.removeAll()
            liveRequests.removeAll()
            defer { waiters.removeAll() }
            return waiters.values.map(\.continuation)
        }
        // The scripted runs see they are no longer active and publish nothing more.
        stranded.forEach { $0.resume(returning: .deny) }
        publish(nil, nil, .resync(oldest: lock.withLock { seq } + 1))
    }

    /// Every `sendMessage` throws `error` until it is set back to nil.
    public func failSends(with error: CoreError?) { lock.withLock { sendFailure = error } }

    /// Every `cancel` throws `error` until it is set back to nil.
    public func failCancels(with error: CoreError?) { lock.withLock { cancelFailure = error } }

    /// Sets what `/api/state` serves as `model` and `model_label` (`servesLabel: false` omits the label, as an older core does).
    public func setModel(_ model: String, label: String?, servesLabel: Bool = true) {
        with { $0.activeModel = model; $0.modelLabel = label; $0.servesModelLabel = servesLabel }
    }

    /// Adds the long demo conversation (`PreviewDemo.longSession`) so a screen can show a transcript that
    /// overflows. It is not part of the sample data, so no other test sees it.
    public func seedLongConversation(id: String = "s-long") {
        let session = PreviewDemo.longSession(id: id, now: now)
        with { $0.sessions[id] = session }
    }

    /// Replaces the "never" list with `PreviewDemo.longNevers`, so the Permissions page can show a list that overflows.
    public func seedLongNevers() { with { $0.nevers = PreviewDemo.longNevers } }

    /// Makes the next `removeNever` fail with `error`, as a core that can't write `config.toml` would.
    public func failNextNeverRemoval(with error: CoreError) { with { $0.neverRemovalFailure = error } }

    /// Makes the next `connectSource` or `disconnectSource` fail with `error`, as a core that can't reach a mail server would.
    public func failNextSourceChange(with error: CoreError) { with { $0.sourceFailure = error } }
    /// The next `settings()` read fails (the rows can't be refreshed after a connect).
    public func failNextSettingsRead(with error: CoreError) { with { $0.settingsFailure = error } }

    /// Marks a provider as having (or lacking) an OAuth client in this build.
    public func setOAuthConfigured(_ provider: OAuthProviderID, _ configured: Bool) {
        with { d in if let i = d.oauth.firstIndex(where: { $0.provider == provider }) { d.oauth[i].clientConfigured = configured } }
    }

    /// The signed-in "browser" finishes the sign-in after this many `oauthAccounts()` polls (default 2). Nil: never.
    public func completeSignIn(afterPolls polls: Int?, address: String = "sam@example.com") {
        with { $0.oauthPollsLeft = polls; $0.oauthAddress = address }
    }

    public func failNextOAuthStart(with error: CoreError) { with { $0.oauthStartFailure = error } }
    public func failNextOAuthSync(with error: CoreError) { with { $0.oauthSyncFailure = error } }

    /// Providers `syncOAuth` was called for, oldest first.
    public var oauthSyncs: [OAuthProviderID] { with { $0.oauthSyncs } }

    /// Every request `connectSource` has accepted or refused, oldest first.
    public var sourceRequests: [SourceConnection] { with { $0.sourceRequests } }

    /// Adds a short conversation whose reply holds very long tool output (`PreviewDemo.longToolOutput`).
    public func seedLongToolOutputConversation(id: String = "s-tool") {
        let session = PreviewDemo.toolOutputSession(id: id, now: now)
        with { $0.sessions[id] = session }
    }

    /// Adds a delivered Inbox item whose body (`PreviewDemo.longInboxBody`) is far taller than the detail pane.
    public func seedLongInboxItem(id: Int = 100) {
        let item = InboxItem(id: id, timestamp: now, title: "Quarterly review results", body: PreviewDemo.longInboxBody, read: false)
        with { $0.inbox.insert(item, at: 0) }
    }

    // MARK: State and conversations

    public func state() async throws -> CoreState {
        with { d in
            CoreState(name: d.persona.name, version: "1.0.0-preview", model: d.activeModel, modelLabel: d.servesModelLabel ? (d.modelLabel ?? d.activeModel) : nil, mode: d.mode,
                      workspace: d.workspace, paused: d.paused, waiting: d.parked.filter { $0.status == "pending" }.count,
                      unread: d.inbox.filter { !$0.read }.count, memories: d.memories.count, onboarded: d.onboarded,
                      riskNoticeAccepted: d.riskNotice.acceptedVersion == d.riskNotice.version,
                      sandbox: d.sandbox, hasKey: true)
        }
    }

    /// As the core lists them: no empty sessions, pinned first, then the most recent.
    public func sessions() async throws -> [SessionSummary] { listed(archived: false) }

    public func archivedSessions() async throws -> [SessionSummary] { listed(archived: true) }

    private func listed(archived wanted: Bool) -> [SessionSummary] {
        lock.withLock {
            data.sessions.values
                .filter { $0.summary.messages > 0 && archived.contains($0.summary.id) == wanted }
                .sorted { $0.summary.updatedAt > $1.summary.updatedAt }
                .map { s in
                    var summary = s.summary
                    summary.pinned = pinned.contains(summary.id)
                    summary.archived = wanted
                    return summary
                }
                .enumerated().sorted { ($0.element.pinned ? 0 : 1, $0.offset) < ($1.element.pinned ? 0 : 1, $1.offset) }.map(\.element)
        }
    }

    public func updateSession(id: String, title: String?, pinned pin: Bool?, archived archive: Bool?) async throws {
        let name = title?.trimmingCharacters(in: .whitespacesAndNewlines)
        if let name, name.isEmpty || name.count > 120 { throw CoreError.failed(name.isEmpty ? "A conversation needs a name." : "Keep the name to 120 characters.") }
        try lock.withLock {
            guard data.sessions[id] != nil else { throw CoreError.notFound }
            if let name { data.sessions[id]?.summary.title = name; data.sessions[id]?.detail.title = name }
            if let pin { if pin { pinned.insert(id) } else { pinned.remove(id) } }
            if let archive { if archive { archived.insert(id) } else { archived.remove(id) } }
        }
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
        lock.withLock {
            data.sessions.removeValue(forKey: id)
            pinned.remove(id)
            archived.remove(id)
        }
    }

    public func sendMessage(sessionID: String?, message: String, title: String?) async throws -> ChatStart {
        if let failure = lock.withLock({ sendFailure }) { throw failure }
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
        if let hook = lock.withLock({ beforeSendReturns }) { await hook(sid) }
        return ChatStart(sessionID: sid, runID: runID)
    }

    public func cancel(sessionID: String) async throws -> Bool {
        if let failure = lock.withLock({ cancelFailure }) { throw failure }
        let wasActive = lock.withLock { activeRuns.contains(sessionID) }
        guard wasActive else { return false }
        // Release a run that waits for an approval, then end it.
        let ids = lock.withLock { Array(waiters.keys) }
        for id in ids where lock.withLock({ waiters[id]?.sid == sessionID }) { resolveWaiter(id, .deny, event: "cancelled") }
        // As on the core: the stopped reply is kept unfinished, so the conversation reopens as `interrupted`.
        lock.withLock {
            activeRuns.remove(sessionID)
            data.sessions[sessionID]?.detail.interrupted = true
        }
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

    public func liveApprovals() async throws -> [ApprovalRequest] {
        lock.withLock { liveRequests.values.sorted { $0.id < $1.id } }
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
        try with { d in
            if let failure = d.settingsFailure { d.settingsFailure = nil; throw failure }
            return CoreSettings(workspace: d.workspace, mode: d.mode, ruleNotices: d.ruleNotices,
                         allowSubscriptionUnattended: d.allowSubscriptionUnattended, connections: d.connections,
                         awareness: d.awareness, phoneConnected: false, cliModel: d.cliModel)
        }
    }

    public func setUnattendedSubscription(_ allowed: Bool) async throws { with { $0.allowSubscriptionUnattended = allowed } }
    public func setCLIModel(_ model: String) async throws { with { $0.cliModel = model } }
    public func setAwareness(_ enabled: Bool) async throws { with { $0.awareness = enabled } }
    public func grants() async throws -> [Grant] { with { $0.grants } }

    public func revokeGrant(rule: String) async throws {
        try with { d in
            guard d.grants.contains(where: { $0.rule == rule }) else { throw CoreError.notFound }
            d.grants.removeAll { $0.rule == rule }
        }
    }

    public func nevers() async throws -> [NeverRule] { with { $0.nevers } }

    public func removeNever(rule: String) async throws {
        try with { d in
            if let failure = d.neverRemovalFailure { d.neverRemovalFailure = nil; throw failure }
            guard d.nevers.contains(where: { $0.rule == rule }) else { throw CoreError.notFound }
            d.nevers.removeAll { $0.rule == rule }
        }
    }

    /// Checks the fields as the core does (same sentences), then marks the row connected.
    public func connectSource(_ source: SourceConnection) async throws {
        try with { d in
            d.sourceRequests.append(source)
            if let failure = d.sourceFailure { d.sourceFailure = nil; throw failure }
            func filled(_ text: String) -> Bool { !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
            let detail: String
            switch source {
            case .documents(let path):
                guard filled(path) else { throw CoreError.failed("Enter the folder that holds your notes or documents.") }
                detail = "\(countPhrase(1, "folder")), \(countPhrase(12, "file")) indexed"
            case .calendar(let url):
                guard filled(url) else { throw CoreError.failed("Paste your calendar's private ICS link.") }
                detail = "\(countPhrase(8, "item")) indexed"
            case .email(let host, let username, let password, _, _):
                guard filled(host) else { throw CoreError.failed("Enter your mail server, for example imap.gmail.com.") }
                guard filled(username) else { throw CoreError.failed("Enter your email address.") }
                guard filled(password) else { throw CoreError.failed("Enter the app password for this account.") }
                detail = username
            case .computer:
                detail = "\(countPhrase(20, "item")) indexed"
            case .localCalendar(let events):
                detail = "\(countPhrase(events.count, "event")) indexed"
            case .localMail(let messages):
                detail = "\(countPhrase(messages.count, "message")) indexed"
            }
            guard let i = d.connections.firstIndex(where: { $0.id == source.sourceID }) else { throw CoreError.notFound }
            d.connections[i].connected = true
            d.connections[i].detail = detail
        }
    }

    public func disconnectSource(id: String) async throws {
        try with { d in
            if let failure = d.sourceFailure { d.sourceFailure = nil; throw failure }
            guard let i = d.connections.firstIndex(where: { $0.id == id }) else { throw CoreError.notFound }
            d.connections[i].connected = false
            d.connections[i].detail = "Not connected"
        }
    }

    public func oauthAccounts() async throws -> [OAuthAccount] {
        with { d in
            if let provider = d.pendingOAuth, let left = d.oauthPollsLeft {
                if left <= 1 {
                    if let i = d.oauth.firstIndex(where: { $0.provider == provider }) { d.oauth[i].connected = true; d.oauth[i].account = d.oauthAddress }
                    d.pendingOAuth = nil
                    d.oauthPollsLeft = nil
                } else {
                    d.oauthPollsLeft = left - 1
                }
            }
            return d.oauth
        }
    }

    public func startOAuth(provider: OAuthProviderID) async throws -> URL {
        try with { d in
            if let failure = d.oauthStartFailure { d.oauthStartFailure = nil; throw failure }
            guard d.oauth.first(where: { $0.provider == provider })?.clientConfigured == true else {
                throw CoreError.failed("This build has no public desktop OAuth client ID.")
            }
            d.pendingOAuth = provider
            return URL(string: "https://accounts.example.com/consent?provider=\(provider.rawValue)")!
        }
    }

    public func syncOAuth(provider: OAuthProviderID) async throws {
        try with { d in
            d.oauthSyncs.append(provider)
            if let failure = d.oauthSyncFailure { d.oauthSyncFailure = nil; throw failure }
        }
    }

    public func disconnectOAuth(provider: OAuthProviderID) async throws {
        try with { d in
            guard let i = d.oauth.firstIndex(where: { $0.provider == provider && $0.connected }) else { throw CoreError.notFound }
            d.oauth[i].connected = false
            d.oauth[i].account = nil
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
            lock.withLock { liveRequests[request.id] = request }
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
        let waiter = lock.withLock { () -> (sid: String, continuation: CheckedContinuation<ApprovalDecision, Never>)? in
            liveRequests.removeValue(forKey: id)
            return waiters.removeValue(forKey: id)
        }
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
