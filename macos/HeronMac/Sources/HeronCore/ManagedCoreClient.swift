import Foundation

/// The runtime client: a `CoreClientProtocol` that follows the core process.
///
/// The stores are built before the core has printed its endpoint, and a restart gives the core a new port
/// and token, so this forwards each call to a `CoreClient` made for the current endpoint:
/// * a call made while the core is starting **waits** for its endpoint (it doesn't fail, or the first
///   `state()` would fail and first run would be skipped);
/// * once the core has stopped for good, calls fail with a clear message instead of waiting;
/// * `events(since:)` reconnects through whichever endpoint is current, so a restart shows up as the
///   `resync` the new core's bus sends for the old position.
public final class ManagedCoreClient: CoreClientProtocol, @unchecked Sendable {
    private let process: CoreProcess
    private let makeClient: @Sendable (CoreEndpoint) -> CoreClient
    private let retryDelay: Duration

    private let lock = NSLock()
    private var client: CoreClient?
    private var waiting: [UUID: CheckedContinuation<CoreClient, Error>] = [:]
    private var down: String?
    private var watcher: Task<Void, Never>?

    public init(process: CoreProcess,
                retryDelay: Duration = .seconds(1),
                makeClient: @escaping @Sendable (CoreEndpoint) -> CoreClient = { CoreClient(endpoint: $0) }) {
        self.process = process
        self.makeClient = makeClient
        self.retryDelay = retryDelay
        let statuses = process.statuses()
        watcher = Task { [weak self] in
            for await status in statuses {
                guard let self else { return }
                self.apply(status)
            }
        }
    }

    deinit { watcher?.cancel() }

    /// Until the first endpoint line, a placeholder; afterwards the current core's.
    public var endpoint: CoreEndpoint {
        lock.withLock { client?.endpoint } ?? CoreEndpoint(port: 0, token: "", backgroundOwner: true, version: "")
    }

    private func apply(_ status: CoreProcess.Status) {
        // Decide under the lock, resume outside it.
        let (woken, outcome): ([CheckedContinuation<CoreClient, Error>], Result<CoreClient, Error>?) = lock.withLock {
            switch status {
            case .starting:
                client = nil
                down = nil
                return ([], nil)
            case .running(let e), .restarted(let e):
                let c = makeClient(e)
                client = c
                down = nil
                defer { waiting.removeAll() }
                return (Array(waiting.values), .success(c))
            case .stopped(let details):
                client = nil
                down = details
                defer { waiting.removeAll() }
                return (Array(waiting.values), .failure(CoreError.failed(details)))
            }
        }
        if let outcome { woken.forEach { $0.resume(with: outcome) } }
    }

    /// The client for the current endpoint, waiting for the core to start if it hasn't yet.
    func current() async throws -> CoreClient {
        let id = UUID()
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                let immediate: Result<CoreClient, Error>? = lock.withLock {
                    if let client { return .success(client) }
                    if let down { return .failure(CoreError.failed(down)) }
                    waiting[id] = continuation
                    return nil
                }
                if let immediate { continuation.resume(with: immediate) }
            }
        } onCancel: {
            let c = lock.withLock { waiting.removeValue(forKey: id) }
            c?.resume(throwing: CancellationError())
        }
    }

    // MARK: Events

    public func events(since: Int?) -> AsyncStream<CoreEvent> {
        EventStream(retryDelay: retryDelay, connect: { [self] since in
            try await current().openEventConnection(since: since)
        }).events(since: since)
    }

    // MARK: Forwarding

    public func state() async throws -> CoreState { try await current().state() }
    public func sessions() async throws -> [SessionSummary] { try await current().sessions() }
    public func session(id: String) async throws -> SessionDetail { try await current().session(id: id) }
    public func deleteSession(id: String) async throws { try await current().deleteSession(id: id) }
    public func sendMessage(sessionID: String?, message: String, title: String?) async throws -> ChatStart {
        try await current().sendMessage(sessionID: sessionID, message: message, title: title)
    }
    public func cancel(sessionID: String) async throws -> Bool { try await current().cancel(sessionID: sessionID) }
    public func approve(id: String, decision: ApprovalDecision) async throws { try await current().approve(id: id, decision: decision) }
    public func approvals() async throws -> [ParkedApproval] { try await current().approvals() }
    public func answerParked(id: Int, action: ParkedAction) async throws { try await current().answerParked(id: id, action: action) }
    public func setMode(_ mode: PermissionMode) async throws { try await current().setMode(mode) }
    public func setPaused(_ paused: Bool) async throws { try await current().setPaused(paused) }
    public func today() async throws -> TodaySummary { try await current().today() }
    public func inbox() async throws -> [InboxItem] { try await current().inbox() }
    public func markInboxRead() async throws { try await current().markInboxRead() }
    public func suggestions() async throws -> [Suggestion] { try await current().suggestions() }
    public func answerSuggestion(id: Int, answer: SuggestionAnswer) async throws { try await current().answerSuggestion(id: id, answer: answer) }
    public func memories(query: String?) async throws -> [Memory] { try await current().memories(query: query) }
    public func addMemory(content: String, kind: String) async throws -> Int { try await current().addMemory(content: content, kind: kind) }
    public func confirmMemory(id: Int) async throws { try await current().confirmMemory(id: id) }
    public func deleteMemory(id: Int) async throws { try await current().deleteMemory(id: id) }
    public func tasks() async throws -> [TaskItem] { try await current().tasks() }
    public func addTask(title: String, due: String?) async throws -> Int { try await current().addTask(title: title, due: due) }
    public func setTaskDone(id: Int, done: Bool) async throws { try await current().setTaskDone(id: id, done: done) }
    public func deleteTask(id: Int) async throws { try await current().deleteTask(id: id) }
    public func schedules() async throws -> [ScheduleItem] { try await current().schedules() }
    public func runSchedule(id: Int) async throws { try await current().runSchedule(id: id) }
    public func setScheduleEnabled(id: Int, enabled: Bool) async throws { try await current().setScheduleEnabled(id: id, enabled: enabled) }
    public func audit() async throws -> AuditLog { try await current().audit() }
    public func persona() async throws -> Persona { try await current().persona() }
    public func setPersona(name: String?, text: String?) async throws -> Persona { try await current().setPersona(name: name, text: text) }
    public func riskNotice() async throws -> RiskNotice { try await current().riskNotice() }
    public func acceptRiskNotice(version: Int) async throws { try await current().acceptRiskNotice(version: version) }
    public func providers() async throws -> [ProviderStatus] { try await current().providers() }
    public func acceptProviderVersion(id: String, version: String) async throws { try await current().acceptProviderVersion(id: id, version: version) }
    public func selectProvider(id: String) async throws { try await current().selectProvider(id: id) }
    public func setOnboarded() async throws { try await current().setOnboarded() }
    public func settings() async throws -> CoreSettings { try await current().settings() }
    public func setUnattendedSubscription(_ allowed: Bool) async throws { try await current().setUnattendedSubscription(allowed) }
    public func setAwareness(_ enabled: Bool) async throws { try await current().setAwareness(enabled) }
    public func grants() async throws -> [Grant] { try await current().grants() }
    public func revokeGrant(rule: String) async throws { try await current().revokeGrant(rule: rule) }
    public func nevers() async throws -> [NeverRule] { try await current().nevers() }
    public func removeNever(rule: String) async throws { try await current().removeNever(rule: rule) }
    public func usage() async throws -> UsageReport { try await current().usage() }
}
