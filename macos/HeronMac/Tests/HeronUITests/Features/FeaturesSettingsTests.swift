import XCTest
import HeronCore
@testable import HeronUI

/// Records the calls a store makes and answers from a script.
private final class FakeFeaturesClient: FeaturesClientProtocol, @unchecked Sendable {
    private let lock = NSLock()
    private var items: [CoreFeature]
    private(set) var sets: [(key: String, on: Bool)] = []
    var failLoad = false
    var failSet: CoreError?

    init(_ items: [CoreFeature]) { self.items = items }

    func features() async throws -> [CoreFeature] {
        if failLoad { throw CoreError.failed("down") }
        return lock.withLock { items }
    }

    func setFeature(key: String, on: Bool) async throws -> Bool {
        if let failSet { throw failSet }
        return try lock.withLock {
            sets.append((key, on))
            guard let i = items.firstIndex(where: { $0.key == key }) else { throw CoreError.notFound }
            items[i].on = on
            return on
        }
    }
}

/// Stands in for `UNUserNotificationCenter`: the real center isn't used from `swift test`.
private final class RecordingPoster: NotificationPoster, @unchecked Sendable {
    private let lock = NSLock()
    private var _requests = 0
    private var _posted: [String] = []
    var grants = true

    var requests: Int { lock.withLock { _requests } }
    var posted: [String] { lock.withLock { _posted } }

    func requestAuthorization() async -> Bool { lock.withLock { _requests += 1 }; return grants }
    func isAuthorized() async -> Bool { grants }
    func post(title: String, body: String, identifier: String) async throws { lock.withLock { _posted.append(identifier) } }
}

@MainActor
final class FeaturesSettingsTests: XCTestCase {
    private let sample = [
        CoreFeature(key: "ask_reasons", title: "Explain approvals", on: true),
        CoreFeature(key: "shortcuts", title: "Run your Shortcuts", on: false),
        CoreFeature(key: "email_drafts", title: "Email drafts", on: false),
    ]

    private func defaults() -> UserDefaults { UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")! }

    private func store(_ client: FakeFeaturesClient, rows: [FeatureRow] = [], defaults: UserDefaults? = nil,
                       poster: RecordingPoster = RecordingPoster(),
                       permissionRequests: [String: @Sendable () async -> Bool] = [:]) -> FeaturesSettingsStore {
        FeaturesSettingsStore(client: client, rows: rows, defaults: defaults ?? self.defaults(), poster: poster,
                              permissionRequests: permissionRequests)
    }

    private let appRow = FeatureRow(key: "quick_entry", title: "Quick entry", kind: .app(defaultsKey: "heron.test.quickEntry"))
    private let coreRow = FeatureRow(key: "shortcuts", title: "Run your Shortcuts", kind: .core(key: "shortcuts"),
                                     permission: "Automation", settingsPane: URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"))

    // MARK: The recorded route

    /// `tests/fixtures/api/features.json`, written from a live core, found from this file's path:
    /// Features → HeronUITests → Tests → HeronMac → macos → repository root.
    private func fixture() throws -> Data {
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<6 { url.deleteLastPathComponent() }
        return try Data(contentsOf: url.appendingPathComponent("tests/fixtures/api/features.json"))
    }

    func testTheRecordedFixtureDecodesAndHasAsManyFeaturesAsItsJSON() throws {
        let data = try fixture()
        let raw = try XCTUnwrap(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        let listed = try XCTUnwrap(raw["features"] as? [[String: Any]])
        let decoded = try FeaturesWire.features(data)
        XCTAssertEqual(decoded.map(\.key), listed.compactMap { $0["key"] as? String })
        XCTAssertEqual(decoded.map(\.on), listed.compactMap { $0["on"] as? Bool })
    }

    func testTheWireShapeDecodesKeyTitleAndOn() throws {
        let json = #"{"features":[{"key":"ask_reasons","title":"Explain approvals","on":true},{"key":"shortcuts","title":"Run your Shortcuts","on":false}]}"#
        let decoded = try FeaturesWire.features(Data(json.utf8))
        XCTAssertEqual(decoded, [CoreFeature(key: "ask_reasons", title: "Explain approvals", on: true),
                                 CoreFeature(key: "shortcuts", title: "Run your Shortcuts", on: false)])
        let saved = try FeaturesWire.saved(Data(#"{"key":"shortcuts","on":true}"#.utf8))
        XCTAssertEqual(saved.key, "shortcuts")
        XCTAssertTrue(saved.on)
    }

    func testAFieldTheCoreStopsSendingFailsLoudly() {
        XCTAssertThrowsError(try FeaturesWire.features(Data(#"{"features":[{"key":"a","title":"A"}]}"#.utf8)))
        XCTAssertThrowsError(try FeaturesWire.saved(Data(#"{"key":"a"}"#.utf8)))
    }

    // MARK: Listing

    func testWithNoRowsTheListIsTheCoreKeysUnderTheCoreTitlesInOrder() async {
        let s = store(FakeFeaturesClient(sample))
        XCTAssertEqual(s.items, [], "nothing shows before the core answers")
        await s.load()
        XCTAssertEqual(s.loadState, .loaded)
        XCTAssertEqual(s.items.map(\.row.title), ["Explain approvals", "Run your Shortcuts", "Email drafts"])
        XCTAssertEqual(s.items.map(\.on), [true, false, false])
        XCTAssertTrue(s.items.allSatisfy { $0.available })
    }

    func testACoreRowTakesItsStateFromTheCoreAndAnAppRowFromDefaults() async {
        let d = defaults()
        d.set(true, forKey: "heron.test.quickEntry")
        let s = store(FakeFeaturesClient(sample), rows: [appRow, coreRow], defaults: d)
        await s.load()
        XCTAssertEqual(s.items.map(\.row.key), ["quick_entry", "shortcuts", "ask_reasons", "email_drafts"],
                       "rows first; a core key that has a row isn't listed twice")
        XCTAssertEqual(s.items.map(\.on), [true, false, true, false])
    }

    func testWhenTheCoreIsDownItsRowsAreUnavailableAndAppRowsStillWork() async {
        let client = FakeFeaturesClient(sample)
        client.failLoad = true
        let s = store(client, rows: [appRow, coreRow])
        await s.load()
        XCTAssertEqual(s.loadState, .failed("down"))
        XCTAssertEqual(s.items.map(\.available), [true, false])
        let changed = await s.set(coreRow, on: true)
        XCTAssertFalse(changed, "a core row can't be flipped blind")
        XCTAssertTrue(client.sets.isEmpty)
        let appChanged = await s.set(appRow, on: true)
        XCTAssertTrue(appChanged)
        XCTAssertEqual(s.items.first?.on, true)
    }

    // MARK: Switching

    func testSwitchingACoreKeyPostsItAndShowsTheSavedState() async throws {
        let client = FakeFeaturesClient(sample)
        let s = store(client)
        await s.load()
        let shortcuts = try XCTUnwrap(s.items.first { $0.row.key == "shortcuts" }).row
        let changed = await s.set(shortcuts, on: true)
        XCTAssertTrue(changed)
        XCTAssertEqual(client.sets.map { $0.key }, ["shortcuts"])
        XCTAssertEqual(client.sets.map { $0.on }, [true])
        XCTAssertEqual(s.items.first { $0.row.key == "shortcuts" }?.on, true)
        XCTAssertEqual(s.items.first { $0.row.key == "ask_reasons" }?.on, true, "other keys are untouched")
    }

    func testAFailedSwitchKeepsTheOldStateAndSaysWhy() async throws {
        let client = FakeFeaturesClient(sample)
        let s = store(client)
        await s.load()
        client.failSet = .failed("The core said no.")
        let row = try XCTUnwrap(s.items.first { $0.row.key == "email_drafts" }).row
        let changed = await s.set(row, on: true)
        XCTAssertFalse(changed)
        XCTAssertEqual(s.items.first { $0.row.key == "email_drafts" }?.on, false)
        XCTAssertEqual(s.errorMessage, "The core said no.")
        client.failSet = nil
        _ = await s.set(row, on: true)
        XCTAssertNil(s.errorMessage, "the next success clears it")
    }

    func testAnAppRowWritesItsDefaultsKey() async {
        let d = defaults()
        let s = store(FakeFeaturesClient([]), rows: [appRow], defaults: d)
        XCTAssertEqual(s.items.first?.on, false)
        _ = await s.set(appRow, on: true)
        XCTAssertTrue(d.bool(forKey: "heron.test.quickEntry"))
        XCTAssertEqual(s.items.first?.on, true)
        _ = await s.set(appRow, on: false)
        XCTAssertFalse(d.bool(forKey: "heron.test.quickEntry"))
    }

    // MARK: Permissions

    func testTurningOnARowThatNeedsNotificationsAsksThroughThePoster() async {
        let poster = RecordingPoster()
        let row = FeatureRow(key: "runs", title: "Run notifications", kind: .app(defaultsKey: "heron.test.runs"),
                             permission: FeatureRow.notificationsPermission)
        let d = defaults()
        let s = store(FakeFeaturesClient([]), rows: [row], defaults: d, poster: poster)
        _ = await s.set(row, on: true)
        XCTAssertEqual(poster.requests, 1)
        XCTAssertTrue(d.bool(forKey: "heron.test.runs"))
    }

    func testARefusedPermissionLeavesTheRowOffAndMarksItDeniedUntilItIsGranted() async {
        let poster = RecordingPoster()
        poster.grants = false
        let row = FeatureRow(key: "runs", title: "Run notifications", kind: .app(defaultsKey: "heron.test.runs"),
                             permission: FeatureRow.notificationsPermission,
                             settingsPane: URL(string: "x-apple.systempreferences:com.apple.Notifications-Settings.extension"))
        let d = defaults()
        let s = store(FakeFeaturesClient([]), rows: [row], defaults: d, poster: poster)
        let changed = await s.set(row, on: true)
        XCTAssertFalse(changed)
        XCTAssertFalse(d.bool(forKey: "heron.test.runs"))
        XCTAssertEqual(s.items.first?.denied, true)
        poster.grants = true
        _ = await s.set(row, on: true)
        XCTAssertEqual(s.items.first?.denied, false)
        XCTAssertEqual(s.items.first?.on, true)
    }

    func testARefusedPermissionDoesNotPostTheCoreKey() async {
        let client = FakeFeaturesClient(sample)
        let s = store(client, rows: [coreRow], permissionRequests: ["Automation": { false }])
        await s.load()
        let changed = await s.set(coreRow, on: true)
        XCTAssertFalse(changed)
        XCTAssertTrue(client.sets.isEmpty, "the core is told only after macOS agrees")
        XCTAssertEqual(s.items.first?.denied, true)
    }

    func testTurningARowOffNeverAsksForAPermission() async {
        let poster = RecordingPoster()
        let row = FeatureRow(key: "runs", title: "Run notifications", kind: .app(defaultsKey: "heron.test.runs"),
                             permission: FeatureRow.notificationsPermission)
        let d = defaults()
        d.set(true, forKey: "heron.test.runs")
        let s = store(FakeFeaturesClient([]), rows: [row], defaults: d, poster: poster)
        _ = await s.set(row, on: false)
        XCTAssertEqual(poster.requests, 0)
        XCTAssertFalse(d.bool(forKey: "heron.test.runs"))
    }

    func testTheRecorderRecordsWhatIsPosted() async throws {
        let poster = RecordingPoster()
        let poster2: any NotificationPoster = poster
        try await poster2.post(title: "Done", body: "Your run finished.", identifier: "run-1")
        XCTAssertEqual(poster.posted, ["run-1"])
    }
}
