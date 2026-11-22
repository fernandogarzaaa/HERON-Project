import Foundation

/// One core feature key as `GET /api/features` reports it (heron-v1-features.md, "Switches").
public struct CoreFeature: Sendable, Equatable, Identifiable {
    public var key: String
    public var title: String
    public var on: Bool
    public var id: String { key }
    public init(key: String, title: String, on: Bool) {
        self.key = key
        self.title = title
        self.on = on
    }
}

/// The two feature routes. Kept apart from `CoreClientProtocol` so a feature's unit can use them
/// without touching the one protocol every client implements.
public protocol FeaturesClientProtocol: Sendable {
    func features() async throws -> [CoreFeature]                              // GET /api/features
    /// `POST /api/features/{key}` with `{"on": bool}`. Returns the `on` the core saved. Throws `CoreError.notFound`
    /// for a key the core doesn't know (404).
    func setFeature(key: String, on: Bool) async throws -> Bool                // POST /api/features/{key}
}

/// Decoding of the two routes' bodies. `GET` answers `{"features": [{"key", "title", "on"}]}` and `POST`
/// answers `{"key", "on"}`. Required fields throw, as in `CoreWire`.
public enum FeaturesWire {
    public static func features(_ data: Data) throws -> [CoreFeature] {
        struct DTO: Decodable { var key: String; var title: String; var on: Bool }
        struct Envelope: Decodable { var features: [DTO] }
        return try JSONDecoder().decode(Envelope.self, from: data).features.map { CoreFeature(key: $0.key, title: $0.title, on: $0.on) }
    }

    public static func saved(_ data: Data) throws -> (key: String, on: Bool) {
        struct DTO: Decodable { var key: String; var on: Bool }
        let dto = try JSONDecoder().decode(DTO.self, from: data)
        return (dto.key, dto.on)
    }
}

/// `URLSession` over the core's loopback API for the two feature routes, with the `x-heron-token` header.
/// The endpoint is read for each request, so the client follows the core across a restart.
public final class FeaturesClient: FeaturesClientProtocol, Sendable {
    private let endpoint: @Sendable () -> CoreEndpoint
    private let urlSession: URLSession

    public init(endpoint: @escaping @Sendable () -> CoreEndpoint, session: URLSession = CoreClient.makeSession()) {
        self.endpoint = endpoint
        self.urlSession = session
    }

    public convenience init(endpoint: CoreEndpoint, session: URLSession = CoreClient.makeSession()) {
        self.init(endpoint: { endpoint }, session: session)
    }

    public func features() async throws -> [CoreFeature] {
        try FeaturesWire.features(try await send("GET", "/features"))
    }

    public func setFeature(key: String, on: Bool) async throws -> Bool {
        let saved = try FeaturesWire.saved(try await send("POST", "/features/\(Self.segment(key))", body: ["on": on]))
        guard saved.key == key else { throw CoreError.failed("The core saved \(saved.key) instead of \(key).") }
        return saved.on
    }

    private static func segment(_ value: String) -> String {
        value.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed.subtracting(CharacterSet(charactersIn: "/"))) ?? value
    }

    private func send(_ method: String, _ path: String, body: [String: Any]? = nil) async throws -> Data {
        let endpoint = endpoint()
        var components = URLComponents()
        components.scheme = "http"
        components.host = "127.0.0.1"
        components.port = endpoint.port
        components.percentEncodedPath = "/api" + path
        guard let url = components.url else { throw CoreError.failed("Can't build the feature request.") }
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.setValue(endpoint.token, forHTTPHeaderField: "x-heron-token")
        if let body {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
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

/// In-memory features for previews and tests that need no core: the three core keys, with the defaults the spec names.
public final class PreviewFeaturesClient: FeaturesClientProtocol, @unchecked Sendable {
    private let lock = NSLock()
    private var items: [CoreFeature]

    public init(items: [CoreFeature] = [
        CoreFeature(key: "ask_reasons", title: "Explain approvals", on: true),
        CoreFeature(key: "shortcuts", title: "Run your Shortcuts", on: false),
        CoreFeature(key: "email_drafts", title: "Email drafts", on: false),
    ]) {
        self.items = items
    }

    public func features() async throws -> [CoreFeature] { lock.withLock { items } }

    public func setFeature(key: String, on: Bool) async throws -> Bool {
        try lock.withLock {
            guard let i = items.firstIndex(where: { $0.key == key }) else { throw CoreError.notFound }
            items[i].on = on
            return on
        }
    }
}
