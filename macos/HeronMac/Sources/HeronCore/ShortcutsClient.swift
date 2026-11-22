import Foundation

/// What `GET /api/shortcuts` reports (heron-v1-features.md, F5): the owner's shortcuts, and the ones Heron may run.
public struct ShortcutsList: Sendable, Equatable {
    /// False when the core can't reach `/usr/bin/shortcuts`; `shortcuts` is then empty.
    public var available: Bool
    public var shortcuts: [String]
    public var allowed: [String]
    public init(available: Bool, shortcuts: [String], allowed: [String]) {
        self.available = available
        self.shortcuts = shortcuts
        self.allowed = allowed
    }
}

/// The two answers the shortcuts routes give that the picker tells apart from any other failure.
public enum ShortcutsError: Error, Sendable, Equatable {
    /// `409 {"error":"feature_off"}`: the core key `shortcuts` is off.
    case featureOff
    /// `400 {"error":"unknown_shortcut"}`: a name isn't in `shortcuts list` (renamed or deleted since the list was read).
    case unknownShortcut(String)
}

/// The two shortcuts routes. Kept apart from `CoreClientProtocol`, like `FeaturesClientProtocol`.
public protocol ShortcutsClientProtocol: Sendable {
    /// `GET /api/shortcuts`. Throws `ShortcutsError.featureOff` while the core key is off.
    func shortcuts() async throws -> ShortcutsList
    /// `POST /api/shortcuts/allowed` with `{"names": [...]}`. Returns the allowlist the core saved.
    /// Throws `ShortcutsError.featureOff` or `.unknownShortcut`.
    func setAllowed(_ names: [String]) async throws -> [String]
}

/// Decoding of the two routes' bodies. Required fields throw, as in `FeaturesWire`.
public enum ShortcutsWire {
    public static func list(_ data: Data) throws -> ShortcutsList {
        struct DTO: Decodable { var available: Bool; var shortcuts: [String]; var allowed: [String] }
        let dto = try JSONDecoder().decode(DTO.self, from: data)
        return ShortcutsList(available: dto.available, shortcuts: dto.shortcuts, allowed: dto.allowed)
    }

    public static func saved(_ data: Data) throws -> [String] {
        struct DTO: Decodable { var allowed: [String] }
        return try JSONDecoder().decode(DTO.self, from: data).allowed
    }

    /// The error for a non-2xx answer: the two the picker handles, else what `CoreClient` makes of it.
    static func failure(status: Int, body: Data) -> Error {
        let object = (try? JSONSerialization.jsonObject(with: body)) as? [String: Any]
        switch (status, object?["error"] as? String) {
        case (409, "feature_off"): return ShortcutsError.featureOff
        case (400, "unknown_shortcut"): return ShortcutsError.unknownShortcut((object?["message"] as? String) ?? "That shortcut isn't in Shortcuts.")
        default: return CoreClient.error(status: status, body: body)
        }
    }
}

/// `URLSession` over the core's loopback API for the two shortcuts routes, with the `x-heron-token` header.
/// The endpoint is read for each request, so the client follows the core across a restart.
public final class ShortcutsClient: ShortcutsClientProtocol, Sendable {
    private let endpoint: @Sendable () -> CoreEndpoint
    private let urlSession: URLSession

    public init(endpoint: @escaping @Sendable () -> CoreEndpoint, session: URLSession = CoreClient.makeSession()) {
        self.endpoint = endpoint
        self.urlSession = session
    }

    public convenience init(endpoint: CoreEndpoint, session: URLSession = CoreClient.makeSession()) {
        self.init(endpoint: { endpoint }, session: session)
    }

    public func shortcuts() async throws -> ShortcutsList {
        try ShortcutsWire.list(try await send("GET", "/shortcuts"))
    }

    public func setAllowed(_ names: [String]) async throws -> [String] {
        try ShortcutsWire.saved(try await send("POST", "/shortcuts/allowed", body: ["names": names]))
    }

    private func send(_ method: String, _ path: String, body: [String: Any]? = nil) async throws -> Data {
        let endpoint = endpoint()
        var components = URLComponents()
        components.scheme = "http"
        components.host = "127.0.0.1"
        components.port = endpoint.port
        components.percentEncodedPath = "/api" + path
        guard let url = components.url else { throw CoreError.failed("Can't build the shortcuts request.") }
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
        guard (200..<300).contains(http.statusCode) else { throw ShortcutsWire.failure(status: http.statusCode, body: data) }
        return data
    }
}

/// In-memory shortcuts for previews and tests that need no core: three shortcuts and an empty allowlist.
public final class PreviewShortcutsClient: ShortcutsClientProtocol, @unchecked Sendable {
    private let lock = NSLock()
    private let names: [String]
    private var allowed: [String]

    public init(names: [String] = ["Add Reminder", "Add Calendar Event", "Get Notes"], allowed: [String] = []) {
        self.names = names
        self.allowed = allowed
    }

    public func shortcuts() async throws -> ShortcutsList {
        lock.withLock { ShortcutsList(available: true, shortcuts: names, allowed: allowed) }
    }

    public func setAllowed(_ requested: [String]) async throws -> [String] {
        try lock.withLock {
            var saved: [String] = []
            for name in requested {
                guard names.contains(name) else { throw ShortcutsError.unknownShortcut("There is no shortcut called \"\(name)\" in Shortcuts.") }
                if !saved.contains(name) { saved.append(name) }
            }
            allowed = saved
            return saved
        }
    }
}
