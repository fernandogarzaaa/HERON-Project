import Foundation

/// One run as `GET /api/runs` lists it (heron-v1-features.md, F3): the record without its two lists
/// (`checkpoints` and `audit_ids`, which F4 reads).
public struct RunRecord: Sendable, Equatable, Identifiable {
    public var runID: String
    public var sessionID: String
    /// The session's title: the owner's own words.
    public var title: String
    /// Where the run's session came from: `user`, `telegram`, `schedule`, `automation` or `proactive`.
    public var origin: String
    /// `running`, `waiting` (for an approval), `done`, `error` or `cancelled`.
    public var status: String
    public var started: Date
    public var ended: Date?
    public var toolCalls: Int

    public var id: String { runID }

    public init(runID: String, sessionID: String, title: String, origin: String, status: String,
                started: Date, ended: Date? = nil, toolCalls: Int = 0) {
        self.runID = runID
        self.sessionID = sessionID
        self.title = title
        self.origin = origin
        self.status = status
        self.started = started
        self.ended = ended
        self.toolCalls = toolCalls
    }

    public var isWaiting: Bool { status == "waiting" }
    public var isFinished: Bool { ended != nil }

    /// Runs from these origins already send their own notice, or the owner is on the phone: the app posts no
    /// "finished" notice for them (heron-v1-features.md, "Notifications have one owner").
    public static let originsWithoutFinishedNotice: Set<String> = ["schedule", "automation", "proactive", "telegram"]

    public var sendsFinishedNotice: Bool { !Self.originsWithoutFinishedNotice.contains(origin) }
}

/// The one route F3 adds that the app reads. Kept apart from `CoreClientProtocol`, as `FeaturesClientProtocol` is,
/// so every client of the core doesn't have to implement it.
public protocol RunsClientProtocol: Sendable {
    func runs() async throws -> [RunRecord]                                    // GET /api/runs
}

/// Decoding of `GET /api/runs`: `{"runs": [{run_id, session_id, title, origin, status, started, ended, tool_calls}]}`.
/// Required fields throw, as in `CoreWire`; `ended` is null while a run goes on.
public enum RunsWire {
    public static func runs(_ data: Data) throws -> [RunRecord] {
        struct DTO: Decodable {
            var runId: String
            var sessionId: String
            var title: String
            var origin: String
            var status: String
            var started: Int
            var ended: Int?
            var toolCalls: Int
        }
        struct Envelope: Decodable { var runs: [DTO] }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(Envelope.self, from: data).runs.map { dto in
            RunRecord(runID: dto.runId, sessionID: dto.sessionId, title: dto.title, origin: dto.origin, status: dto.status,
                      started: Date(timeIntervalSince1970: TimeInterval(dto.started)),
                      ended: dto.ended.map { Date(timeIntervalSince1970: TimeInterval($0)) },
                      toolCalls: dto.toolCalls)
        }
    }
}

/// `URLSession` over the core's loopback API for `GET /api/runs`, with the `x-heron-token` header.
/// The endpoint is read for each request, so the client follows the core across a restart.
public final class RunsClient: RunsClientProtocol, Sendable {
    private let endpoint: @Sendable () -> CoreEndpoint
    private let urlSession: URLSession

    public init(endpoint: @escaping @Sendable () -> CoreEndpoint, session: URLSession = CoreClient.makeSession()) {
        self.endpoint = endpoint
        self.urlSession = session
    }

    public convenience init(endpoint: CoreEndpoint, session: URLSession = CoreClient.makeSession()) {
        self.init(endpoint: { endpoint }, session: session)
    }

    public func runs() async throws -> [RunRecord] {
        let endpoint = endpoint()
        var components = URLComponents()
        components.scheme = "http"
        components.host = "127.0.0.1"
        components.port = endpoint.port
        components.percentEncodedPath = "/api/runs"
        guard let url = components.url else { throw CoreError.failed("Can't build the runs request.") }
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue(endpoint.token, forHTTPHeaderField: "x-heron-token")
        let result: (Data, URLResponse)
        do {
            result = try await urlSession.data(for: request)
        } catch is CancellationError {
            throw CancellationError()
        } catch {
            throw CoreError.failed("Can't reach Heron's core: \(error.localizedDescription)")
        }
        let (data, response) = result
        guard let http = response as? HTTPURLResponse else { throw CoreError.failed("The core sent something that isn't HTTP.") }
        guard (200..<300).contains(http.statusCode) else { throw CoreClient.error(status: http.statusCode, body: data) }
        return try RunsWire.runs(data)
    }
}

/// In-memory runs for previews and tests that need no core.
public final class PreviewRunsClient: RunsClientProtocol, @unchecked Sendable {
    private let lock = NSLock()
    private var items: [RunRecord]
    private var failure: CoreError?

    public init(runs: [RunRecord] = []) { self.items = runs }

    public func setRuns(_ runs: [RunRecord]) { lock.withLock { items = runs } }
    public func setFailure(_ error: CoreError?) { lock.withLock { failure = error } }

    public func runs() async throws -> [RunRecord] {
        let (current, error) = lock.withLock { (items, failure) }
        if let error { throw error }
        return current
    }
}

/// An event source that can ask the core to hand its notices to the app (`GET /api/events?notify=1`).
/// Kept apart from `CoreClientProtocol` so the one protocol every client implements doesn't change.
public protocol NotifyingEventsClient: Sendable {
    /// `notify == true` opens the stream with `?notify=1`: while one such stream is open, the core posts no
    /// notice of its own and sends `notification` events instead.
    func events(since: Int?, notify: Bool) -> AsyncStream<CoreEvent>
}
