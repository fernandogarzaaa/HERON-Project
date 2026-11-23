import Foundation

/// Everything the app asks of the core. The views and stores talk only to this protocol; U4a backs it
/// with `PreviewCoreClient` stubs and U4b adds the `URLSession` client. Each method maps to one route
/// of the API in heron-v1.md §3 and §4 (`GET /api/state` is `state()`, and so on).
public protocol CoreClientProtocol: Sendable {
    /// What the endpoint line said. `backgroundOwner == false` shows the "another Heron" banner.
    var endpoint: CoreEndpoint { get }

    // State and conversations
    func state() async throws -> CoreState                                   // GET /api/state
    func sessions() async throws -> [SessionSummary]                         // GET /api/sessions
    func session(id: String) async throws -> SessionDetail                   // GET /api/sessions/{id}
    func deleteSession(id: String) async throws                              // DELETE /api/sessions/{id}
    func archivedSessions() async throws -> [SessionSummary]                 // GET /api/sessions?archived=true
    /// Rename, pin or archive one conversation; nil leaves that part as it is.
    func updateSession(id: String, title: String?, pinned: Bool?, archived: Bool?) async throws // POST /api/sessions/{id}

    /// `POST /api/chat`. Returns once the run has started (its `session` event arrived); the run's events
    /// then arrive on `events(since:)`. Throws `CoreError.sessionBusy` (409) or `.riskNoticeNotAccepted` (428).
    func sendMessage(sessionID: String?, message: String, title: String?) async throws -> ChatStart
    func cancel(sessionID: String) async throws -> Bool                      // POST /api/cancel
    /// `GET /api/events?since=`. One stream of every session's events.
    func events(since: Int?) -> AsyncStream<CoreEvent>

    // Approvals
    func approve(id: String, decision: ApprovalDecision) async throws        // POST /api/approve
    func approvals() async throws -> [ParkedApproval]                        // GET /api/approvals
    func liveApprovals() async throws -> [ApprovalRequest]                   // GET /api/approvals/live
    func answerParked(id: Int, action: ParkedAction) async throws            // POST /api/approvals/{id}

    // Mode and pause
    func setMode(_ mode: PermissionMode) async throws                        // POST /api/mode
    func setPaused(_ paused: Bool) async throws                              // POST /api/pause

    // Today and inbox
    func today() async throws -> TodaySummary                                // GET /api/today
    func inbox() async throws -> [InboxItem]                                 // GET /api/inbox
    func markInboxRead() async throws                                        // POST /api/inbox/read
    func suggestions() async throws -> [Suggestion]                          // GET /api/suggestions
    func answerSuggestion(id: Int, answer: SuggestionAnswer) async throws    // POST /api/suggestions/{id}

    // Memory, tasks, schedules, audit
    func memories(query: String?) async throws -> [Memory]                   // GET /api/memories
    func addMemory(content: String, kind: String) async throws -> Int        // POST /api/memories
    func confirmMemory(id: Int) async throws                                 // POST /api/memories/{id}/confirm
    func deleteMemory(id: Int) async throws                                  // DELETE /api/memories/{id}
    func tasks() async throws -> [TaskItem]                                  // GET /api/tasks
    func addTask(title: String, due: String?) async throws -> Int            // POST /api/tasks
    func setTaskDone(id: Int, done: Bool) async throws                       // POST /api/tasks/{id}
    func deleteTask(id: Int) async throws                                    // DELETE /api/tasks/{id}
    func schedules() async throws -> [ScheduleItem]                          // GET /api/schedules
    func runSchedule(id: Int) async throws                                   // POST /api/schedules/{id}/run
    func setScheduleEnabled(id: Int, enabled: Bool) async throws             // POST /api/schedules/{id}
    func audit() async throws -> AuditLog                                    // GET /api/audit

    // Persona and first run
    func persona() async throws -> Persona                                   // GET /api/persona
    func setPersona(name: String?, text: String?) async throws -> Persona    // POST /api/persona
    func riskNotice() async throws -> RiskNotice                             // GET /api/risk-notice
    /// `POST /api/risk-notice {"version": n}`. Throws `CoreError.riskNoticeOutdated` on 409.
    func acceptRiskNotice(version: Int) async throws
    func providers() async throws -> [ProviderStatus]                        // GET /api/providers
    func acceptProviderVersion(id: String, version: String) async throws     // POST /api/providers/{id}/accept-version
    func selectProvider(id: String) async throws                             // POST /api/settings
    func setOnboarded() async throws                                         // POST /api/onboarded

    // Settings
    func settings() async throws -> CoreSettings                             // GET /api/settings
    func setUnattendedSubscription(_ allowed: Bool) async throws             // POST /api/settings
    func setCLIModel(_ model: String) async throws                           // POST /api/settings
    func setAwareness(_ enabled: Bool) async throws                          // POST /api/awareness
    func grants() async throws -> [Grant]                                    // GET /api/grants
    func revokeGrant(rule: String) async throws                              // DELETE /api/grants
    func nevers() async throws -> [NeverRule]                                // GET /api/nevers
    /// `DELETE /api/nevers {"rule"}`. Throws `CoreError.notFound` (404) for a rule that isn't a "never".
    func removeNever(rule: String) async throws                              // DELETE /api/nevers
    /// `POST /api/sources/{name}`. Returns once the core has proved the sign-in (email) or finished the first sync;
    /// throws `CoreError.failed` with the core's user-facing sentence ("Heron could not find that folder…").
    func connectSource(_ source: SourceConnection) async throws
    /// `DELETE /api/sources/{name}`: disconnects and forgets what came from it.
    func disconnectSource(id: String) async throws
    /// `GET /api/oauth`: Google and Microsoft, whether each is connected and whether this build can sign in to it.
    func oauthAccounts() async throws -> [OAuthAccount]
    /// `POST /api/oauth/{provider}/start`: the consent URL to open in the system browser. The core finishes the
    /// sign-in itself when the browser reaches its loopback callback; the app polls `oauthAccounts()` for it.
    func startOAuth(provider: OAuthProviderID) async throws -> URL
    /// `POST /api/oauth/{provider}/sync`: reads the account's mail and calendar now.
    func syncOAuth(provider: OAuthProviderID) async throws
    /// `DELETE /api/oauth/{provider}`: signs out and forgets what came from the account.
    func disconnectOAuth(provider: OAuthProviderID) async throws
    func usage() async throws -> UsageReport                                // GET /api/usage
}

/// The `session` event that starts a run.
public struct ChatStart: Sendable, Equatable {
    public var sessionID: String
    public var runID: String
    public init(sessionID: String, runID: String) {
        self.sessionID = sessionID
        self.runID = runID
    }
}
