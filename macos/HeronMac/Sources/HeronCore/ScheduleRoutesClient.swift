import Foundation

/// A schedule as the Morning brief switch needs it: its id, and the name it was made under.
public struct ScheduleRef: Sendable, Equatable, Identifiable {
    public var id: Int
    public var name: String
    public init(id: Int, name: String) {
        self.id = id
        self.name = name
    }
}

/// The three schedule routes F6's switch uses: list, create with a body the shared protocol can't carry (the
/// optional `workflow` field of `POST /api/schedules`), and delete. Kept apart from `CoreClientProtocol` so a
/// feature's unit can use them without touching the one protocol every client implements.
public protocol ScheduleRoutesClientProtocol: Sendable {
    func scheduleRefs() async throws -> [ScheduleRef]                  // GET /api/schedules
    /// `POST /api/schedules` with this JSON body. Returns the new schedule's id.
    func createSchedule(jsonBody: Data) async throws -> Int            // POST /api/schedules
    func deleteSchedule(id: Int) async throws                          // DELETE /api/schedules/{id}
}

/// Decoding of the two bodies: `GET` answers an array of schedules (only `id` and `name` are read) and `POST`
/// answers `{"id": n}`. Required fields throw, as in `CoreWire`.
public enum ScheduleRoutesWire {
    public static func refs(_ data: Data) throws -> [ScheduleRef] {
        struct DTO: Decodable { var id: Int; var name: String }
        return try JSONDecoder().decode([DTO].self, from: data).map { ScheduleRef(id: $0.id, name: $0.name) }
    }

    public static func created(_ data: Data) throws -> Int {
        struct DTO: Decodable { var id: Int }
        return try JSONDecoder().decode(DTO.self, from: data).id
    }
}

/// `URLSession` over the core's loopback API for the schedule routes, with the `x-heron-token` header.
/// The endpoint is read for each request, so the client follows the core across a restart.
public final class ScheduleRoutesClient: ScheduleRoutesClientProtocol, Sendable {
    private let endpoint: @Sendable () -> CoreEndpoint
    private let urlSession: URLSession

    public init(endpoint: @escaping @Sendable () -> CoreEndpoint, session: URLSession = CoreClient.makeSession()) {
        self.endpoint = endpoint
        self.urlSession = session
    }

    public convenience init(endpoint: CoreEndpoint, session: URLSession = CoreClient.makeSession()) {
        self.init(endpoint: { endpoint }, session: session)
    }

    public func scheduleRefs() async throws -> [ScheduleRef] {
        try ScheduleRoutesWire.refs(try await send("GET", "/schedules"))
    }

    public func createSchedule(jsonBody: Data) async throws -> Int {
        try ScheduleRoutesWire.created(try await send("POST", "/schedules", body: jsonBody))
    }

    public func deleteSchedule(id: Int) async throws {
        _ = try await send("DELETE", "/schedules/\(id)")
    }

    private func send(_ method: String, _ path: String, body: Data? = nil) async throws -> Data {
        let endpoint = endpoint()
        var components = URLComponents()
        components.scheme = "http"
        components.host = "127.0.0.1"
        components.port = endpoint.port
        components.percentEncodedPath = "/api" + path
        guard let url = components.url else { throw CoreError.failed("Can't build the schedule request.") }
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.setValue(endpoint.token, forHTTPHeaderField: "x-heron-token")
        if let body {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = body
        }
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
        return data
    }
}

/// In-memory schedules for previews and tests that need no core.
public final class PreviewScheduleRoutesClient: ScheduleRoutesClientProtocol, @unchecked Sendable {
    private let lock = NSLock()
    private var items: [ScheduleRef] = []
    private var nextID = 1

    public init() {}

    public func scheduleRefs() async throws -> [ScheduleRef] { lock.withLock { items } }

    public func createSchedule(jsonBody: Data) async throws -> Int {
        guard let object = try JSONSerialization.jsonObject(with: jsonBody) as? [String: Any], let name = object["name"] as? String else {
            throw CoreError.failed("The schedule has no name.")
        }
        return lock.withLock {
            let id = nextID
            nextID += 1
            items.append(ScheduleRef(id: id, name: name))
            return id
        }
    }

    public func deleteSchedule(id: Int) async throws {
        try lock.withLock {
            guard let i = items.firstIndex(where: { $0.id == id }) else { throw CoreError.notFound }
            items.remove(at: i)
        }
    }
}
