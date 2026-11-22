import XCTest
import HeronCore
@testable import HeronUI

/// Answers from a script and records every save.
private final class FakeShortcutsClient: ShortcutsClientProtocol, @unchecked Sendable {
    private let lock = NSLock()
    var list: Result<ShortcutsList, Error>
    var saveResult: Result<[String], Error>?
    /// Runs inside `setAllowed`, before it answers: lets a test tick again while the save is in flight.
    var duringSave: (@Sendable () async -> Void)?
    private var _saves: [[String]] = []

    var saves: [[String]] { lock.withLock { _saves } }

    init(_ list: ShortcutsList) { self.list = .success(list) }

    func shortcuts() async throws -> ShortcutsList { try list.get() }

    func setAllowed(_ names: [String]) async throws -> [String] {
        lock.withLock { _saves.append(names) }
        await duringSave?()
        return try (saveResult ?? .success(names)).get()
    }
}

@MainActor
final class ShortcutsPickerTests: XCTestCase {
    private let three = ShortcutsList(available: true, shortcuts: ["Get Notes", "Add Reminder", "Send Message"], allowed: ["Get Notes"])

    private func loaded(_ client: FakeShortcutsClient) async -> ShortcutsPickerStore {
        let store = ShortcutsPickerStore(client: client)
        await store.load()
        return store
    }

    // MARK: The row

    func testFeatureRowsListsShortcutsOnceAsACoreRowThatNeedsNoPermission() throws {
        let row = try XCTUnwrap(FeatureRows.all.first { $0.key == "shortcuts" })
        XCTAssertEqual(row.title, "Run your Shortcuts")
        XCTAssertEqual(row.kind, .core(key: "shortcuts"))
        XCTAssertNil(row.permission, "F5's TCC answer is none for Heron")
        XCTAssertEqual(FeatureRows.all.filter { $0.key == "shortcuts" }.count, 1)
    }

    // MARK: Loading

    func testLoadShowsTheShortcutsInTheCoresOrderWithTheAllowedOnesTicked() async {
        let store = await loaded(FakeShortcutsClient(three))
        XCTAssertEqual(store.phase, .ready)
        XCTAssertEqual(store.shortcuts, ["Get Notes", "Add Reminder", "Send Message"])
        XCTAssertEqual(store.shortcuts.map { store.isAllowed($0) }, [true, false, false])
        XCTAssertTrue(store.missing.isEmpty)
    }

    func testTheRecordedFixtureDecodesToWhatTheStoreShows() async throws {
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<6 { url.deleteLastPathComponent() }
        let data = try Data(contentsOf: url.appendingPathComponent("tests/fixtures/api/shortcuts.json"))
        let list = try ShortcutsWire.list(data)
        let store = await loaded(FakeShortcutsClient(list))
        XCTAssertEqual(store.phase, .ready)
        XCTAssertEqual(store.shortcuts, ["Get Notes", "Add Reminder", "Send Message"])
        XCTAssertEqual(store.allowed, ["Get Notes"])
    }

    func testFeatureOffIsItsOwnStateAndShowsNoShortcuts() async {
        let client = FakeShortcutsClient(three)
        client.list = .failure(ShortcutsError.featureOff)
        let store = await loaded(client)
        XCTAssertEqual(store.phase, .featureOff)
        XCTAssertTrue(store.shortcuts.isEmpty)
        XCTAssertTrue(store.allowed.isEmpty)
    }

    func testUnavailableIsItsOwnStateAndOffersNothingToTick() async {
        let store = await loaded(FakeShortcutsClient(ShortcutsList(available: false, shortcuts: [], allowed: ["Get Notes"])))
        XCTAssertEqual(store.phase, .unavailable)
        XCTAssertTrue(store.shortcuts.isEmpty)
        let changed = await store.set("Get Notes", allowed: false)
        XCTAssertFalse(changed, "nothing can be changed while Shortcuts isn't there")
    }

    func testACoreThatDoesNotAnswerFailsWithItsMessageAndAReloadRecovers() async {
        let client = FakeShortcutsClient(three)
        client.list = .failure(CoreError.failed("down"))
        let store = await loaded(client)
        XCTAssertEqual(store.phase, .failed("down"))
        client.list = .success(three)
        await store.load()
        XCTAssertEqual(store.phase, .ready)
        XCTAssertEqual(store.shortcuts.count, 3)
    }

    func testAnAllowedNameShortcutsNoLongerListsIsReportedAsMissing() async {
        let store = await loaded(FakeShortcutsClient(ShortcutsList(available: true, shortcuts: ["Get Notes"], allowed: ["Get Notes", "Old One"])))
        XCTAssertEqual(store.missing, ["Old One"])
    }

    // MARK: Ticking

    func testTickingSavesTheWholeAllowlistInTheListsOrderAndShowsWhatTheCoreSaved() async {
        let client = FakeShortcutsClient(three)
        let store = await loaded(client)
        let changed = await store.set("Send Message", allowed: true)
        XCTAssertTrue(changed)
        XCTAssertEqual(client.saves, [["Get Notes", "Send Message"]])
        XCTAssertEqual(store.allowed, ["Get Notes", "Send Message"])
        XCTAssertNil(store.errorMessage)
    }

    func testUntickingTheLastShortcutSavesAnEmptyList() async {
        let client = FakeShortcutsClient(three)
        let store = await loaded(client)
        let changed = await store.set("Get Notes", allowed: false)
        XCTAssertTrue(changed)
        XCTAssertEqual(client.saves, [[]])
        XCTAssertTrue(store.allowed.isEmpty)
    }

    func testTickingWhatIsAlreadyTickedSavesNothing() async {
        let client = FakeShortcutsClient(three)
        let store = await loaded(client)
        let changed = await store.set("Get Notes", allowed: true)
        XCTAssertFalse(changed)
        XCTAssertTrue(client.saves.isEmpty)
    }

    func testANameThatIsNotListedIsNeverSent() async {
        let client = FakeShortcutsClient(three)
        let store = await loaded(client)
        let changed = await store.set("Invented", allowed: true)
        XCTAssertFalse(changed)
        XCTAssertTrue(client.saves.isEmpty)
    }

    func testAFailedSaveKeepsTheOldTicksAndSaysWhy() async {
        let client = FakeShortcutsClient(three)
        let store = await loaded(client)
        client.saveResult = .failure(CoreError.failed("The core said no."))
        let changed = await store.set("Add Reminder", allowed: true)
        XCTAssertFalse(changed)
        XCTAssertEqual(store.allowed, ["Get Notes"])
        XCTAssertEqual(store.errorMessage, "The core said no.")
        XCTAssertFalse(store.isSaving)
        client.saveResult = nil
        let retried = await store.set("Add Reminder", allowed: true)
        XCTAssertTrue(retried)
        XCTAssertNil(store.errorMessage, "the next success clears it")
    }

    func testASaveForAShortcutThatWasRenamedReloadsTheListAndTellsTheOwner() async {
        let client = FakeShortcutsClient(three)
        let store = await loaded(client)
        client.saveResult = .failure(ShortcutsError.unknownShortcut("There is no shortcut called \"Add Reminder\" in Shortcuts."))
        client.list = .success(ShortcutsList(available: true, shortcuts: ["Get Notes", "Add Reminders"], allowed: ["Get Notes"]))
        let changed = await store.set("Add Reminder", allowed: true)
        XCTAssertFalse(changed)
        XCTAssertEqual(store.shortcuts, ["Get Notes", "Add Reminders"], "the list is read again")
        XCTAssertEqual(store.errorMessage, "There is no shortcut called \"Add Reminder\" in Shortcuts.")
    }

    func testTheCoreBeingSwitchedOffMidSaveMovesToFeatureOff() async {
        let client = FakeShortcutsClient(three)
        let store = await loaded(client)
        client.saveResult = .failure(ShortcutsError.featureOff)
        let changed = await store.set("Add Reminder", allowed: true)
        XCTAssertFalse(changed)
        XCTAssertEqual(store.phase, .featureOff)
        XCTAssertTrue(store.shortcuts.isEmpty)
    }

    func testSavingDropsANameShortcutsNoLongerListsSoTheCoreDoesNotRefuseIt() async {
        let client = FakeShortcutsClient(ShortcutsList(available: true, shortcuts: ["Get Notes", "Add Reminder"], allowed: ["Get Notes", "Old One"]))
        let store = await loaded(client)
        XCTAssertEqual(store.missing, ["Old One"])
        _ = await store.set("Add Reminder", allowed: true)
        XCTAssertEqual(client.saves, [["Get Notes", "Add Reminder"]])
        XCTAssertTrue(store.missing.isEmpty)
    }

    func testATickWhileASaveIsInFlightIsIgnored() async {
        let client = FakeShortcutsClient(three)
        let store = await loaded(client)
        let second = Box<Bool?>(nil)
        client.duringSave = { @Sendable in
            let changed = await store.set("Add Reminder", allowed: true)
            second.value = changed
        }
        let first = await store.set("Send Message", allowed: true)
        XCTAssertTrue(first)
        XCTAssertEqual(second.value, false)
        XCTAssertEqual(client.saves, [["Get Notes", "Send Message"]], "one save, so saves can't land out of order")
        XCTAssertFalse(store.isSaving)
    }

    // MARK: The preview client

    func testThePreviewClientRefusesAnUnknownNameLikeTheCore() async {
        let preview = PreviewShortcutsClient(names: ["A"], allowed: [])
        do {
            _ = try await preview.setAllowed(["B"])
            XCTFail("expected unknownShortcut")
        } catch {
            guard case ShortcutsError.unknownShortcut = error else { return XCTFail("wrong error \(error)") }
        }
        let saved = try? await preview.setAllowed(["A", "A"])
        XCTAssertEqual(saved, ["A"], "a repeated name is saved once")
    }
}

private final class Box<T>: @unchecked Sendable {
    var value: T
    init(_ value: T) { self.value = value }
}
