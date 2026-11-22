import XCTest
import HeronCore
@testable import HeronUI

/// A core that keeps schedules in memory and records what the switch asked of it.
private final class FakeScheduleRoutes: ScheduleRoutesClientProtocol, @unchecked Sendable {
    private let lock = NSLock()
    private var items: [ScheduleRef] = []
    private var nextID = 1
    private var _bodies: [Data] = []
    private var _deleted: [Int] = []
    private var _fail = false

    var bodies: [Data] { lock.withLock { _bodies } }
    var deleted: [Int] { lock.withLock { _deleted } }
    var schedules: [ScheduleRef] { lock.withLock { items } }
    /// While true every call throws, as when the core isn't up.
    var fail: Bool {
        get { lock.withLock { _fail } }
        set { lock.withLock { _fail = newValue } }
    }

    /// A schedule that exists without the switch having made it (the owner's own, or one made by an earlier launch).
    func seed(name: String) {
        lock.withLock {
            items.append(ScheduleRef(id: nextID, name: name))
            nextID += 1
        }
    }

    func scheduleRefs() async throws -> [ScheduleRef] {
        try lock.withLock {
            if _fail { throw CoreError.failed("down") }
            return items
        }
    }

    func createSchedule(jsonBody: Data) async throws -> Int {
        try lock.withLock {
            if _fail { throw CoreError.failed("down") }
            let object = try JSONSerialization.jsonObject(with: jsonBody) as? [String: Any]
            let id = nextID
            nextID += 1
            _bodies.append(jsonBody)
            items.append(ScheduleRef(id: id, name: object?["name"] as? String ?? ""))
            return id
        }
    }

    func deleteSchedule(id: Int) async throws {
        try lock.withLock {
            if _fail { throw CoreError.failed("down") }
            guard let i = items.firstIndex(where: { $0.id == id }) else { throw CoreError.notFound }
            items.remove(at: i)
            _deleted.append(id)
        }
    }
}

private final class AllowingPoster: NotificationPoster, @unchecked Sendable {
    func requestAuthorization() async -> Bool { true }
    func isAuthorized() async -> Bool { true }
    func post(title: String, body: String, identifier: String) async throws {}
}

private final class OffFeaturesClient: FeaturesClientProtocol, @unchecked Sendable {
    func features() async throws -> [CoreFeature] { [] }
    func setFeature(key: String, on: Bool) async throws -> Bool { on }
}

@MainActor
final class BriefTests: XCTestCase {
    private func defaults() -> UserDefaults { UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")! }

    private func decoded(_ data: Data) throws -> [String: Any] {
        try XCTUnwrap(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    /// Every step in a list, with the steps nested under `then` and `else`.
    private func allSteps(_ steps: [[String: Any]]) -> [[String: Any]] {
        steps.flatMap { step -> [[String: Any]] in
            var nested: [[String: Any]] = []
            for branch in ["then", "else"] { nested += allSteps(step[branch] as? [[String: Any]] ?? []) }
            return [step] + nested
        }
    }

    // MARK: The request

    func test_scheduleHasNoModelStep() throws {
        let body = try decoded(try BriefSchedule.requestData())
        XCTAssertEqual(body["name"] as? String, "Morning brief")
        XCTAssertEqual(body["when"] as? String, "daily 08:30")
        XCTAssertEqual(body["prompt"] as? String, "Morning brief")
        let workflow = try XCTUnwrap(body["workflow"] as? [[String: Any]])
        let steps = allSteps(workflow)
        // Q6 holds only steps that call the model. This one has none: counting, a test, and a notice.
        let kinds = steps.compactMap { $0["do"] as? String }
        XCTAssertEqual(kinds.count, steps.count, "every step names what it does")
        XCTAssertEqual(kinds.sorted(), ["if", "notify", "tool"])
        for forbidden in ["ask", "agent"] { XCTAssertFalse(kinds.contains(forbidden), "no \(forbidden) step") }
        XCTAssertEqual(steps.compactMap { $0["tool"] as? String }, ["today_counts"])
    }

    func test_theBodyIsTheOneTheSpecNames() throws {
        let body = try decoded(try BriefSchedule.requestData())
        let workflow = try XCTUnwrap(body["workflow"] as? [[String: Any]])
        XCTAssertEqual(workflow.count, 2)
        XCTAssertEqual(workflow[0]["id"] as? String, "counts")
        XCTAssertEqual((workflow[0]["args"] as? [String: Any])?.count, 0)
        XCTAssertEqual(workflow[1]["id"] as? String, "check")
        XCTAssertEqual(workflow[1]["condition"] as? String, "{{ steps.counts.out | lines | first }} > 0")
        let notice = try XCTUnwrap((workflow[1]["then"] as? [[String: Any]])?.first)
        XCTAssertEqual(notice["id"] as? String, "tell")
        XCTAssertEqual(notice["title"] as? String, "Heron")
        // The lock screen shows the notice, so it carries the second line (counts and fixed words), nothing else.
        XCTAssertEqual(notice["text"] as? String, "{{ steps.counts.out | lines | last }}")
    }

    func test_theStoredTimeIsUsedAndABadOneFallsBack() {
        let d = defaults()
        let brief = BriefController(client: FakeScheduleRoutes(), defaults: d)
        XCTAssertEqual(brief.time, "08:30")
        d.set("07:05", forKey: BriefSchedule.timeKey)
        XCTAssertEqual(brief.time, "07:05")
        for bad in ["8:30", "25:00", "08:60", "tomorrow", "08:30:00", ""] {
            d.set(bad, forKey: BriefSchedule.timeKey)
            XCTAssertEqual(brief.time, "08:30", "\(bad) is not a time")
        }
    }

    // MARK: First launch

    func test_firstLaunchCreatesTheScheduleOnce() async throws {
        let d = defaults()
        let core = FakeScheduleRoutes()
        let brief = BriefController(client: core, defaults: d)
        await brief.createOnFirstLaunch()
        XCTAssertEqual(core.bodies.count, 1)
        XCTAssertEqual(try decoded(core.bodies[0])["name"] as? String, "Morning brief")
        XCTAssertTrue(d.bool(forKey: BriefSchedule.createdKey))
        XCTAssertTrue(d.bool(forKey: BriefSchedule.defaultsKey), "ships on (QF3)")

        // A second launch, and a launch of a fresh controller, send nothing.
        await brief.createOnFirstLaunch()
        await BriefController(client: core, defaults: d).createOnFirstLaunch()
        XCTAssertEqual(core.bodies.count, 1)

        // The owner deletes the schedule; the next launch must not bring it back.
        try await core.deleteSchedule(id: core.schedules[0].id)
        await BriefController(client: core, defaults: d).createOnFirstLaunch()
        XCTAssertEqual(core.bodies.count, 1)
        XCTAssertTrue(core.schedules.isEmpty)
    }

    func test_firstLaunchTriesAgainWhenTheCoreWasDown() async {
        let d = defaults()
        let core = FakeScheduleRoutes()
        core.fail = true
        let brief = BriefController(client: core, defaults: d)
        await brief.createOnFirstLaunch()
        XCTAssertFalse(d.bool(forKey: BriefSchedule.createdKey))
        XCTAssertFalse(d.bool(forKey: BriefSchedule.defaultsKey))
        XCTAssertTrue(core.bodies.isEmpty)

        core.fail = false
        await brief.createOnFirstLaunch()
        XCTAssertEqual(core.bodies.count, 1)
        XCTAssertTrue(d.bool(forKey: BriefSchedule.createdKey))
    }

    func test_firstLaunchDoesNotAdoptTheOwnersSchedule() async {
        let d = defaults()
        let core = FakeScheduleRoutes()
        core.seed(name: "Morning brief")
        await BriefController(client: core, defaults: d).createOnFirstLaunch()
        XCTAssertEqual(core.bodies.count, 1, "the app makes its own; it never takes the owner's by name")
        XCTAssertEqual(core.schedules.count, 2)
        XCTAssertEqual(d.object(forKey: BriefSchedule.scheduleIDKey) as? Int, core.schedules.last?.id)
        XCTAssertTrue(d.bool(forKey: BriefSchedule.createdKey))
    }

    func test_firstLaunchKeepsTheScheduleItMadeWhenItIsStillThere() async throws {
        let d = defaults()
        let core = FakeScheduleRoutes()
        let brief = BriefController(client: core, defaults: d)
        try await brief.set(true)
        d.removeObject(forKey: BriefSchedule.createdKey)
        await brief.createOnFirstLaunch()
        XCTAssertEqual(core.bodies.count, 1, "the stored id still exists, so nothing is made again")
    }

    // MARK: The switch

    func test_switchOffLeavesTheOwnersSameNamedSchedule() async throws {
        let d = defaults()
        let core = FakeScheduleRoutes()
        core.seed(name: "Morning brief")
        let ownersID = core.schedules[0].id
        let brief = BriefController(client: core, defaults: d)
        try await brief.set(true)
        XCTAssertEqual(core.schedules.count, 2)

        try await brief.set(false)
        XCTAssertEqual(core.schedules.map(\.id), [ownersID], "only the app's own schedule is deleted")
        XCTAssertEqual(core.deleted.count, 1)
        XCTAssertNotEqual(core.deleted.first, ownersID)

        // With nothing of the app's left, off touches nothing, and the owner's schedule survives it.
        try await brief.set(false)
        XCTAssertEqual(core.schedules.map(\.id), [ownersID])
        XCTAssertEqual(core.deleted.count, 1)
    }

    func test_switchOffWithNoScheduleOfItsOwnDeletesNothing() async throws {
        let core = FakeScheduleRoutes()
        core.seed(name: "Morning brief")
        try await BriefController(client: core, defaults: defaults()).set(false)
        XCTAssertEqual(core.schedules.count, 1)
        XCTAssertTrue(core.deleted.isEmpty)
    }

    // MARK: The time

    func test_changingTheTimeRecreatesTheSchedule() async throws {
        let d = defaults()
        let core = FakeScheduleRoutes()
        core.seed(name: "Morning brief")
        let ownersID = core.schedules[0].id
        let brief = BriefController(client: core, defaults: d)
        try await brief.set(true)
        let firstID = try XCTUnwrap(d.object(forKey: BriefSchedule.scheduleIDKey) as? Int)

        try await brief.setTime("07:15")
        XCTAssertEqual(brief.time, "07:15")
        XCTAssertEqual(core.deleted, [firstID], "the app's schedule is deleted by its stored id")
        XCTAssertEqual(core.bodies.count, 2)
        XCTAssertEqual(try decoded(core.bodies[1])["when"] as? String, "daily 07:15")
        XCTAssertEqual(core.schedules.count, 2)
        XCTAssertTrue(core.schedules.contains { $0.id == ownersID }, "the owner's schedule is left alone")
        let newID = try XCTUnwrap(d.object(forKey: BriefSchedule.scheduleIDKey) as? Int)
        XCTAssertNotEqual(newID, firstID)
        XCTAssertTrue(core.schedules.contains { $0.id == newID })

        // Off now deletes the new one, by the new id.
        try await brief.set(false)
        XCTAssertEqual(core.schedules.map(\.id), [ownersID])
    }

    func test_aTimeChangeWithNoScheduleOfItsOwnOnlySavesTheTime() async throws {
        let d = defaults()
        let core = FakeScheduleRoutes()
        let brief = BriefController(client: core, defaults: d)
        try await brief.setTime("06:45")
        XCTAssertEqual(brief.time, "06:45")
        XCTAssertTrue(core.bodies.isEmpty)
        // Switching on later uses the saved time.
        try await brief.set(true)
        XCTAssertEqual(try decoded(core.bodies[0])["when"] as? String, "daily 06:45")
    }

    func test_aBadTimeOrARefusedChangeLeavesTheTimeAndScheduleAlone() async throws {
        let d = defaults()
        let core = FakeScheduleRoutes()
        let brief = BriefController(client: core, defaults: d)
        try await brief.set(true)
        let id = core.schedules[0].id
        do { try await brief.setTime("25:00"); XCTFail("expected a refusal") } catch {}
        core.fail = true
        do { try await brief.setTime("07:15"); XCTFail("expected the failure") } catch {}
        core.fail = false
        XCTAssertEqual(brief.time, "08:30")
        XCTAssertEqual(core.schedules.map(\.id), [id])
    }

    func test_theRowsTimePickerChangesTheTimeAndReportsARefusal() async {
        let d = defaults()
        let core = FakeScheduleRoutes()
        let brief = BriefController(client: core, defaults: d)
        let store = FeaturesSettingsStore(client: OffFeaturesClient(), rows: [.morningBrief], defaults: d, poster: AllowingPoster(),
                                          appSwitches: [FeatureRow.morningBrief.key: { on in try await brief.set(on) }], brief: brief)
        XCTAssertEqual(store.briefTime, "08:30")
        await store.set(.morningBrief, on: true)
        let changed = await store.setBriefTime("09:00")
        XCTAssertTrue(changed)
        XCTAssertEqual(store.briefTime, "09:00")
        XCTAssertNil(store.errorMessage)

        core.fail = true
        let refused = await store.setBriefTime("10:00")
        XCTAssertFalse(refused)
        XCTAssertEqual(store.briefTime, "09:00", "the picker goes back to the stored time")
        XCTAssertNotNil(store.errorMessage)
    }

    func test_thePickersDateRoundTripsToTheTime() {
        for t in ["00:00", "08:30", "23:59"] {
            XCTAssertEqual(BriefSchedule.time(from: BriefSchedule.date(fromTime: t)), t)
        }
    }

    func test_switchOffDeletesTheSchedule() async throws {
        let d = defaults()
        let core = FakeScheduleRoutes()
        core.seed(name: "Friday summary")
        let brief = BriefController(client: core, defaults: d)
        try await brief.set(true)
        XCTAssertEqual(core.schedules.map(\.name), ["Friday summary", "Morning brief"])

        try await brief.set(false)
        XCTAssertEqual(core.schedules.map(\.name), ["Friday summary"], "only the brief is deleted")
        XCTAssertEqual(core.deleted.count, 1)

        // Off with nothing to delete is quiet, and back on makes exactly one.
        try await brief.set(false)
        XCTAssertEqual(core.deleted.count, 1)
        try await brief.set(true)
        try await brief.set(true)
        XCTAssertEqual(core.schedules.map(\.name), ["Friday summary", "Morning brief"])
        XCTAssertEqual(core.bodies.count, 2)
    }

    func test_switchingOffNeverMakesFirstLaunchRecreateIt() async throws {
        let d = defaults()
        let core = FakeScheduleRoutes()
        let brief = BriefController(client: core, defaults: d)
        try await brief.set(true)
        try await brief.set(false)
        await BriefController(client: core, defaults: d).createOnFirstLaunch()
        XCTAssertTrue(core.schedules.isEmpty)
    }

    func test_aFailedSwitchThrowsAndLeavesTheScheduleAlone() async throws {
        let core = FakeScheduleRoutes()
        let brief = BriefController(client: core, defaults: defaults())
        try await brief.set(true)
        core.fail = true
        do { try await brief.set(false); XCTFail("expected the failure") } catch {}
        core.fail = false
        XCTAssertEqual(core.schedules.count, 1)
    }

    // MARK: The row

    func test_theRowIsInTheFeaturesList() {
        let row = FeatureRows.all.first { $0.key == "morning_brief" }
        XCTAssertEqual(row?.title, "Morning brief")
        XCTAssertEqual(row?.kind, .app(defaultsKey: BriefSchedule.defaultsKey))
        XCTAssertEqual(row?.permission, FeatureRow.notificationsPermission)
        XCTAssertEqual(FeatureRows.all.filter { $0.key == "morning_brief" }.count, 1)
    }

    private func settings(_ core: FakeScheduleRoutes, defaults d: UserDefaults) -> FeaturesSettingsStore {
        let brief = BriefController(client: core, defaults: d)
        return FeaturesSettingsStore(client: OffFeaturesClient(), rows: [.morningBrief], defaults: d, poster: AllowingPoster(),
                                     appSwitches: [FeatureRow.morningBrief.key: { on in try await brief.set(on) }])
    }

    func test_theRowSwitchMakesAndDeletesTheScheduleAndSavesTheSetting() async {
        let d = defaults()
        let core = FakeScheduleRoutes()
        let store = settings(core, defaults: d)
        let turnedOn = await store.set(.morningBrief, on: true)
        XCTAssertTrue(turnedOn)
        XCTAssertEqual(core.schedules.map(\.name), ["Morning brief"])
        XCTAssertTrue(d.bool(forKey: BriefSchedule.defaultsKey))
        XCTAssertEqual(store.items.first?.on, true)

        let turnedOff = await store.set(.morningBrief, on: false)
        XCTAssertTrue(turnedOff)
        XCTAssertTrue(core.schedules.isEmpty)
        XCTAssertFalse(d.bool(forKey: BriefSchedule.defaultsKey))
        XCTAssertEqual(store.items.first?.on, false)
    }

    func test_theRowStaysOffAndSaysWhyWhenTheCoreRefuses() async {
        let d = defaults()
        let core = FakeScheduleRoutes()
        core.fail = true
        let store = settings(core, defaults: d)
        let turnedOn = await store.set(.morningBrief, on: true)
        XCTAssertFalse(turnedOn)
        XCTAssertFalse(d.bool(forKey: BriefSchedule.defaultsKey))
        XCTAssertEqual(store.items.first?.on, false)
        XCTAssertNotNil(store.errorMessage)
    }

    func test_theListReadsTheSettingTheFirstLaunchRecorded() async {
        let d = defaults()
        let core = FakeScheduleRoutes()
        let store = settings(core, defaults: d)
        XCTAssertEqual(store.items.first?.on, false, "nothing is recorded before the first launch")
        await BriefController(client: core, defaults: d).createOnFirstLaunch()
        await store.load()
        XCTAssertEqual(store.items.first?.on, true)
    }

    // MARK: The routes

    func test_theScheduleRoutesDecodeTheCoresBodies() throws {
        let list = Data(#"[{"id": 4, "name": "Morning brief", "cron": "30 8 * * *", "outline": "tool today_counts", "uses_model": false}, {"id": 5, "name": "Friday summary"}]"#.utf8)
        XCTAssertEqual(try ScheduleRoutesWire.refs(list), [ScheduleRef(id: 4, name: "Morning brief"), ScheduleRef(id: 5, name: "Friday summary")])
        XCTAssertEqual(try ScheduleRoutesWire.created(Data(#"{"id": 9}"#.utf8)), 9)
        XCTAssertThrowsError(try ScheduleRoutesWire.created(Data(#"{"ok": true}"#.utf8)))
        XCTAssertThrowsError(try ScheduleRoutesWire.refs(Data(#"[{"id": 4}]"#.utf8)))
    }
}
