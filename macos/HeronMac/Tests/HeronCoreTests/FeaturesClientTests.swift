import XCTest
@testable import HeronCore

/// The two feature routes against a stubbed transport (`StubProtocol`): the path, the token, the body and the errors.
final class FeaturesClientTests: XCTestCase {
    private let endpoint = CoreEndpoint(port: 4321, token: "tok", backgroundOwner: true, version: "test")

    private func client() -> FeaturesClient { FeaturesClient(endpoint: endpoint, session: StubProtocol.session()) }

    func testGetFeaturesSendsTheTokenAndDecodesTheFixture() async throws {
        let body = try Fixtures.text("features.json")
        StubProtocol.reset { _ in .init(200, json: body) }
        let features = try await client().features()
        XCTAssertEqual(features, try FeaturesWire.features(try Fixtures.data("features.json")))
        let seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual(seen.method, "GET")
        XCTAssertEqual(seen.path, "/api/features")
        XCTAssertEqual(seen.token, "tok")
    }

    func testGetFeaturesDecodesTheTitles() async throws {
        StubProtocol.reset { _ in .init(200, json: #"{"features":[{"key":"shortcuts","title":"Run your Shortcuts","on":true}]}"#) }
        let features = try await client().features()
        XCTAssertEqual(features, [CoreFeature(key: "shortcuts", title: "Run your Shortcuts", on: true)])
    }

    func testPostSendsOnAsJSONAndReturnsTheSavedValue() async throws {
        StubProtocol.reset { _ in .init(200, json: #"{"key":"shortcuts","on":true}"#) }
        let on = try await client().setFeature(key: "shortcuts", on: true)
        XCTAssertTrue(on)
        let seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual(seen.method, "POST")
        XCTAssertEqual(seen.path, "/api/features/shortcuts")
        XCTAssertEqual(seen.token, "tok")
        XCTAssertEqual(seen.contentType, "application/json")
        XCTAssertEqual(seen.body, #"{"on":true}"#)
    }

    func testPostForAnUnknownKeyIsNotFound() async {
        StubProtocol.reset { _ in .init(404, json: #"{"error":"unknown_feature","message":"no such feature"}"#) }
        do {
            _ = try await client().setFeature(key: "nope", on: true)
            XCTFail("expected notFound")
        } catch {
            XCTAssertEqual(error as? CoreError, .notFound)
        }
    }

    func testPostThatEchoesAnotherKeyIsRefused() async {
        StubProtocol.reset { _ in .init(200, json: #"{"key":"other","on":true}"#) }
        do {
            _ = try await client().setFeature(key: "shortcuts", on: true)
            XCTFail("expected a failure")
        } catch {
            guard case CoreError.failed = error else { return XCTFail("wrong error \(error)") }
        }
    }

    func testTheClientFollowsTheCoreToANewEndpoint() async throws {
        StubProtocol.reset { _ in .init(200, json: #"{"features":[]}"#) }
        let box = EndpointBox(endpoint)
        let client = FeaturesClient(endpoint: { box.value }, session: StubProtocol.session())
        _ = try await client.features()
        box.value = CoreEndpoint(port: 9, token: "new", backgroundOwner: true, version: "test")
        _ = try await client.features()
        XCTAssertEqual(StubProtocol.seen.map(\.token), ["tok", "new"])
    }
}

private final class EndpointBox: @unchecked Sendable {
    private let lock = NSLock()
    private var _value: CoreEndpoint
    init(_ value: CoreEndpoint) { _value = value }
    var value: CoreEndpoint {
        get { lock.withLock { _value } }
        set { lock.withLock { _value = newValue } }
    }
}
