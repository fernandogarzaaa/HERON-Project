import Foundation

/// The app's runtime client: `URLSession` over the core's loopback API, with the `x-heron-token` header on
/// every request (heron-v1.md §3 and §4). One instance talks to one core process (one endpoint); see
/// `ManagedCoreClient` for the one that follows the core across a restart.
public final class CoreClient: CoreClientProtocol, Sendable {
    public let endpoint: CoreEndpoint
    private let urlSession: URLSession
    private let base: URL

    /// A session suited to the core: no cache, no cookies, and an idle timeout longer than the core's 15-second keep-alive.
    public static func makeSession() -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 60
        configuration.timeoutIntervalForResource = 24 * 3600
        configuration.waitsForConnectivity = false
        return URLSession(configuration: configuration)
    }

    public init(endpoint: CoreEndpoint, session: URLSession = CoreClient.makeSession()) {
        self.endpoint = endpoint
        self.urlSession = session
        self.base = URL(string: "http://127.0.0.1:\(endpoint.port)")!
    }

    // MARK: Transport

    private func urlRequest(_ method: String, _ path: String, query: [URLQueryItem] = [], body: [String: Any]? = nil) throws -> URLRequest {
        var components = URLComponents(url: base, resolvingAgainstBaseURL: false)!
        components.percentEncodedPath = "/api" + path // segments are already percent-encoded by `segment`
        if !query.isEmpty {
            // Encoded by hand: `URLComponents.queryItems` leaves `&` and `+` in a value as they are.
            let allowed = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")
            components.percentEncodedQuery = query
                .map { "\($0.name)=\(($0.value ?? "").addingPercentEncoding(withAllowedCharacters: allowed) ?? "")" }
                .joined(separator: "&")
        }
        var request = URLRequest(url: components.url!)
        request.httpMethod = method
        request.setValue(endpoint.token, forHTTPHeaderField: "x-heron-token")
        if let body {
            // axum's `Json` extractor rejects a body without this header.
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
        }
        return request
    }

    private func segment(_ value: String) -> String {
        value.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed.subtracting(CharacterSet(charactersIn: "/"))) ?? value
    }

    /// Sends the request and returns the body of a 2xx answer; any other status becomes a `CoreError`.
    @discardableResult
    private func send(_ method: String, _ path: String, query: [URLQueryItem] = [], body: [String: Any]? = nil,
                      conflict: CoreError? = nil) async throws -> Data {
        let (data, response) = try await transport(try urlRequest(method, path, query: query, body: body))
        guard (200..<300).contains(response.statusCode) else { throw Self.error(status: response.statusCode, body: data, conflict: conflict) }
        return data
    }

    private func transport(_ request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        do {
            let (data, response) = try await urlSession.data(for: request)
            guard let http = response as? HTTPURLResponse else { throw CoreError.failed("The core sent something that isn't HTTP.") }
            return (data, http)
        } catch let error as CoreError {
            throw error
        } catch is CancellationError {
            throw CancellationError()
        } catch {
            throw CoreError.failed("Can't reach Heron's core: \(error.localizedDescription)")
        }
    }

    /// Starts a streaming request and returns once the response headers are in.
    private func open(_ request: URLRequest) async throws -> (URLSession.AsyncBytes, HTTPURLResponse) {
        do {
            let (bytes, response) = try await urlSession.bytes(for: request)
            guard let http = response as? HTTPURLResponse else { throw CoreError.failed("The core sent something that isn't HTTP.") }
            return (bytes, http)
        } catch let error as CoreError {
            throw error
        } catch is CancellationError {
            throw CancellationError()
        } catch {
            throw CoreError.failed("Can't reach Heron's core: \(error.localizedDescription)")
        }
    }

    /// Maps a failed answer: `409 session_busy` and `428` are the two a screen reacts to, `404` is "not found",
    /// and everything else carries the core's own message (`{"error": "…"}`).
    static func error(status: Int, body: Data, conflict: CoreError? = nil) -> CoreError {
        let object = (try? JSONSerialization.jsonObject(with: body)) as? [String: Any]
        let code = object?["error"] as? String
        let message = (object?["message"] as? String) ?? code
        switch status {
        case 409:
            if code == "session_busy" { return .sessionBusy }
            return conflict ?? .failed(message ?? "That changed while you were working. Reload and try again.")
        case 428: return .riskNoticeNotAccepted
        case 404: return .notFound
        default: return .failed(message ?? "The core answered \(status).")
        }
    }

    private func object(_ data: Data) throws -> [String: Any] {
        guard let o = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw DecodingError.dataCorrupted(.init(codingPath: [], debugDescription: "expected a JSON object"))
        }
        return o
    }

    // MARK: State and conversations

    public func state() async throws -> CoreState { try CoreWire.state(try await send("GET", "/state")) }
    public func sessions() async throws -> [SessionSummary] { try CoreWire.sessions(try await send("GET", "/sessions")) }
    public func session(id: String) async throws -> SessionDetail { try CoreWire.session(try await send("GET", "/sessions/\(segment(id))")) }
    public func deleteSession(id: String) async throws { try await send("DELETE", "/sessions/\(segment(id))") }

    /// `POST /api/chat` answers with the run's events as SSE. This reads only up to its `session` event and then
    /// drops the response: the run goes on without it (heron-v1.md §3), and its events reach the app once, on `events(since:)`.
    /// The core has no title field on `/api/chat`: `title` is accepted for the protocol and not sent (DESIGN-CHOICES.md).
    public func sendMessage(sessionID: String?, message: String, title: String?) async throws -> ChatStart {
        var body: [String: Any] = ["message": message]
        if let sessionID { body["session_id"] = sessionID }
        let (bytes, http) = try await open(try urlRequest("POST", "/chat", body: body))
        guard (200..<300).contains(http.statusCode) else {
            var data = Data()
            for try await byte in bytes { data.append(byte); if data.count > 64 * 1024 { break } }
            throw Self.error(status: http.statusCode, body: data)
        }
        var parser = SSEParser()
        var line = Data()
        for try await byte in bytes {
            line.append(byte)
            guard byte == 0x0A else { continue }
            let messages = parser.feed(line)
            line.removeAll(keepingCapacity: true)
            for sse in messages {
                guard let event = try? CoreWire.envelope(Data(sse.data.utf8)).event else { continue }
                switch event.kind {
                case .session(let id, let runID): return ChatStart(sessionID: id, runID: runID)
                case .error(let text, _, _, _): throw CoreError.failed(text)
                default: continue
                }
            }
        }
        throw CoreError.failed("The core ended the reply before it started.")
    }

    public func cancel(sessionID: String) async throws -> Bool {
        let data = try await send("POST", "/cancel", body: ["session_id": sessionID])
        return try object(data)["cancelled"] as? Bool ?? false
    }

    public func events(since: Int?) -> AsyncStream<CoreEvent> {
        EventStream(connect: { [self] since in try await openEventConnection(since: since) }).events(since: since)
    }

    /// `GET /api/events`, as raw SSE bytes grouped by line. Throws when the core refuses or isn't there.
    public func openEventConnection(since: Int?) async throws -> AsyncThrowingStream<Data, Error> {
        let query = since.map { [URLQueryItem(name: "since", value: String($0))] } ?? []
        var request = try urlRequest("GET", "/events", query: query)
        request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
        let (bytes, http) = try await open(request)
        guard (200..<300).contains(http.statusCode) else { throw CoreError.failed("The core refused the event stream.") }
        return AsyncThrowingStream { continuation in
            let task = Task {
                do {
                    var line = Data()
                    for try await byte in bytes {
                        line.append(byte)
                        if byte == 0x0A { continuation.yield(line); line.removeAll(keepingCapacity: true) }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    // MARK: Approvals

    public func approve(id: String, decision: ApprovalDecision) async throws {
        try await send("POST", "/approve", body: ["id": id, "decision": decision.rawValue])
    }

    public func approvals() async throws -> [ParkedApproval] { try CoreWire.parkedApprovals(try await send("GET", "/approvals")) }

    public func liveApprovals() async throws -> [ApprovalRequest] { try CoreWire.liveApprovals(try await send("GET", "/approvals/live")) }

    public func answerParked(id: Int, action: ParkedAction) async throws {
        try await send("POST", "/approvals/\(id)", body: ["action": action.rawValue])
    }

    // MARK: Mode and pause

    public func setMode(_ mode: PermissionMode) async throws { try await send("POST", "/mode", body: ["mode": mode.rawValue]) }
    public func setPaused(_ paused: Bool) async throws { try await send("POST", "/pause", body: ["paused": paused]) }

    // MARK: Today and inbox

    public func today() async throws -> TodaySummary { try CoreWire.today(try await send("GET", "/today")) }
    public func inbox() async throws -> [InboxItem] { try CoreWire.inbox(try await send("GET", "/inbox")) }
    public func markInboxRead() async throws { try await send("POST", "/inbox/read", body: [:]) }
    public func suggestions() async throws -> [Suggestion] { try CoreWire.suggestions(try await send("GET", "/suggestions")) }

    public func answerSuggestion(id: Int, answer: SuggestionAnswer) async throws {
        try await send("POST", "/suggestions/\(id)", body: ["action": answer.rawValue])
    }

    // MARK: Memory, tasks, schedules, audit

    public func memories(query: String?) async throws -> [Memory] {
        let q = query?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return try CoreWire.memories(try await send("GET", "/memories", query: q.isEmpty ? [] : [URLQueryItem(name: "q", value: q)]))
    }

    public func addMemory(content: String, kind: String, source: String) async throws -> Int {
        try identifier(try await send("POST", "/memories", body: ["content": content, "kind": kind, "source": source]))
    }

    public func confirmMemory(id: Int) async throws { try await send("POST", "/memories/\(id)/confirm", body: [:]) }
    public func deleteMemory(id: Int) async throws { try await send("DELETE", "/memories/\(id)") }

    public func tasks() async throws -> [TaskItem] { try CoreWire.tasks(try await send("GET", "/tasks")) }

    public func addTask(title: String, due: String?) async throws -> Int {
        var body: [String: Any] = ["title": title]
        if let due, !due.isEmpty { body["due"] = due }
        return try identifier(try await send("POST", "/tasks", body: body))
    }

    public func setTaskDone(id: Int, done: Bool) async throws {
        try await send("POST", "/tasks/\(id)", body: ["status": done ? "done" : "open"])
    }

    public func deleteTask(id: Int) async throws { try await send("DELETE", "/tasks/\(id)") }

    public func schedules() async throws -> [ScheduleItem] { try CoreWire.schedules(try await send("GET", "/schedules")) }
    public func runSchedule(id: Int) async throws { try await send("POST", "/schedules/\(id)/run", body: [:]) }

    /// The core has no route to pause or resume one schedule: `POST /api/schedules/{id}` accepts only `may`
    /// (what the job may do unattended). Rather than send a request that answers 400, this says so (DESIGN-CHOICES.md).
    public func setScheduleEnabled(id: Int, enabled: Bool) async throws {
        throw CoreError.failed("Heron's core can't pause or resume a single schedule yet. You can delete it instead.")
    }

    public func audit() async throws -> AuditLog { try CoreWire.audit(try await send("GET", "/audit")) }

    private func identifier(_ data: Data) throws -> Int {
        guard let id = try object(data)["id"] as? Int else {
            throw DecodingError.keyNotFound(CodingKeyName("id"), .init(codingPath: [], debugDescription: "the core's answer has no id"))
        }
        return id
    }

    // MARK: Persona and first run

    public func persona() async throws -> Persona { try CoreWire.persona(try await send("GET", "/persona")) }

    public func setPersona(name: String?, text: String?) async throws -> Persona {
        var body: [String: Any] = [:]
        if let name { body["name"] = name }
        if let text { body["text"] = text }
        return try CoreWire.persona(try await send("POST", "/persona", body: body))
    }

    public func riskNotice() async throws -> RiskNotice { try CoreWire.riskNotice(try await send("GET", "/risk-notice")) }

    /// `POST /api/risk-notice` answers `204 No Content`, or `409` when the register has a newer version.
    public func acceptRiskNotice(version: Int) async throws {
        try await send("POST", "/risk-notice", body: ["version": version], conflict: .riskNoticeOutdated)
    }

    /// `GET /api/providers` (installed, version, login) joined with `GET /api/settings` (login command, auth kind, which is in use).
    public func providers() async throws -> [ProviderStatus] {
        async let list = send("GET", "/providers")
        async let settings = try? send("GET", "/settings")
        return try CoreWire.providers(try await list, settings: await settings)
    }

    public func acceptProviderVersion(id: String, version: String) async throws {
        try await send("POST", "/providers/\(segment(id))/accept-version", body: ["version": version])
    }

    public func selectProvider(id: String) async throws { try await send("POST", "/settings", body: ["provider": id]) }
    public func setOnboarded() async throws { try await send("POST", "/onboarded", body: [:]) }

    // MARK: Settings

    /// `GET /api/settings`, with the Connections tab's three routes beside it. If one of those fails, the page still loads.
    public func settings() async throws -> CoreSettings {
        async let main = send("GET", "/settings")
        async let sources = try? send("GET", "/sources")
        async let awareness = try? send("GET", "/awareness")
        async let telegram = try? send("GET", "/telegram")
        return try CoreWire.settings(try await main, sources: await sources, awareness: await awareness, telegram: await telegram)
    }

    public func setUnattendedSubscription(_ allowed: Bool) async throws {
        try await send("POST", "/settings", body: ["allow_unattended_subscription": allowed])
    }

    public func setAwareness(_ enabled: Bool) async throws { try await send("POST", "/awareness", body: ["enabled": enabled]) }
    public func grants() async throws -> [Grant] { try CoreWire.grants(try await send("GET", "/grants")) }
    public func revokeGrant(rule: String) async throws { try await send("DELETE", "/grants", body: ["rule": rule]) }
    public func nevers() async throws -> [NeverRule] { try CoreWire.nevers(try await send("GET", "/nevers")) }
    public func removeNever(rule: String) async throws { try await send("DELETE", "/nevers", body: ["rule": rule]) }
    public func usage() async throws -> UsageReport { try CoreWire.usage(try await send("GET", "/usage")) }
}

private struct CodingKeyName: CodingKey {
    var stringValue: String
    var intValue: Int? { nil }
    init(_ s: String) { stringValue = s }
    init?(stringValue: String) { self.stringValue = stringValue }
    init?(intValue: Int) { nil }
}
