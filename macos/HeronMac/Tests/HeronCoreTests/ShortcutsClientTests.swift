import XCTest
@testable import HeronCore

/// The two shortcuts routes against a stubbed transport (`StubProtocol`): the path, the token, the body and the errors.
final class ShortcutsClientTests: XCTestCase {
    private let endpoint = CoreEndpoint(port: 4321, token: "tok", backgroundOwner: true, version: "test")

    private func client() -> ShortcutsClient { ShortcutsClient(endpoint: endpoint, session: StubProtocol.session()) }

    func testGetSendsTheTokenAndDecodesTheRecordedFixture() async throws {
        let body = try Fixtures.text("shortcuts.json")
        StubProtocol.reset { _ in .init(200, json: body) }
        let list = try await client().shortcuts()
        XCTAssertEqual(list, try ShortcutsWire.list(try Fixtures.data("shortcuts.json")))
        XCTAssertEqual(list.shortcuts, ["Get Notes", "Add Reminder", "Send Message"])
        XCTAssertEqual(list.allowed, ["Get Notes"])
        XCTAssertTrue(list.available)
        let seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual(seen.method, "GET")
        XCTAssertEqual(seen.path, "/api/shortcuts")
        XCTAssertEqual(seen.token, "tok")
    }

    func testGetWithoutTheBinaryDecodesAsUnavailable() async throws {
        StubProtocol.reset { _ in .init(200, json: #"{"available":false,"shortcuts":[],"allowed":["Get Notes"]}"#) }
        let list = try await client().shortcuts()
        XCTAssertEqual(list, ShortcutsList(available: false, shortcuts: [], allowed: ["Get Notes"]))
    }

    func testGetWhileTheKeyIsOffThrowsFeatureOff() async {
        StubProtocol.reset { _ in .init(409, json: #"{"error":"feature_off","message":"Run your Shortcuts is switched off in Settings › Features."}"#) }
        do {
            _ = try await client().shortcuts()
            XCTFail("expected featureOff")
        } catch {
            XCTAssertEqual(error as? ShortcutsError, .featureOff)
        }
    }

    func testPostSendsTheNamesAsJSONAndReturnsTheSavedList() async throws {
        StubProtocol.reset { _ in .init(200, json: #"{"allowed":["Add Reminder","Get Notes"]}"#) }
        let saved = try await client().setAllowed(["Add Reminder", "Get Notes"])
        XCTAssertEqual(saved, ["Add Reminder", "Get Notes"])
        let seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual(seen.method, "POST")
        XCTAssertEqual(seen.path, "/api/shortcuts/allowed")
        XCTAssertEqual(seen.token, "tok")
        XCTAssertEqual(seen.contentType, "application/json")
        XCTAssertEqual(seen.body, #"{"names":["Add Reminder","Get Notes"]}"#)
    }

    func testPostForAnUnknownNameThrowsUnknownShortcutWithTheCoresMessage() async {
        StubProtocol.reset { _ in .init(400, json: #"{"error":"unknown_shortcut","message":"There is no shortcut called \"Nope\" in Shortcuts."}"#) }
        do {
            _ = try await client().setAllowed(["Nope"])
            XCTFail("expected unknownShortcut")
        } catch {
            XCTAssertEqual(error as? ShortcutsError, .unknownShortcut("There is no shortcut called \"Nope\" in Shortcuts."))
        }
    }

    func testPostWhileTheKeyIsOffThrowsFeatureOff() async {
        StubProtocol.reset { _ in .init(409, json: #"{"error":"feature_off","message":"off"}"#) }
        do {
            _ = try await client().setAllowed([])
            XCTFail("expected featureOff")
        } catch {
            XCTAssertEqual(error as? ShortcutsError, .featureOff)
        }
    }

    func testAnyOtherFailureIsACoreErrorWithTheCoresMessage() async {
        StubProtocol.reset { _ in .init(500, json: #"{"error":"internal","message":"The shortcuts tool broke."}"#) }
        do {
            _ = try await client().shortcuts()
            XCTFail("expected a failure")
        } catch {
            XCTAssertEqual(error as? CoreError, .failed("The shortcuts tool broke."))
        }
    }

    func testAFieldTheCoreStopsSendingFailsLoudly() {
        XCTAssertThrowsError(try ShortcutsWire.list(Data(#"{"available":true,"shortcuts":[]}"#.utf8)))
        XCTAssertThrowsError(try ShortcutsWire.saved(Data(#"{}"#.utf8)))
    }
}
