import XCTest
import AppKit
import UserNotifications
import HeronCore
@testable import HeronUI

/// Stands in for `UNUserNotificationCenter`, and records what the notifier hands it.
private final class RecordingPoster: NotificationPoster, @unchecked Sendable {
    struct Notice: Equatable { var title: String; var body: String; var identifier: String }

    private let lock = NSLock()
    private var _requests = 0
    private var _posted: [Notice] = []
    var grants = true

    var requests: Int { lock.withLock { _requests } }
    var posted: [Notice] { lock.withLock { _posted } }

    func requestAuthorization() async -> Bool { lock.withLock { _requests += 1 }; return grants }
    func isAuthorized() async -> Bool { grants }
    func post(title: String, body: String, identifier: String) async throws {
        lock.withLock { _posted.append(Notice(title: title, body: body, identifier: identifier)) }
    }
}

/// Records how `AppModel` opened its event stream.
private final class OpenRecorder: @unchecked Sendable {
    private let lock = NSLock()
    private var _calls: [(since: Int?, notify: Bool)] = []
    var calls: [(since: Int?, notify: Bool)] { lock.withLock { _calls } }
    func open(_ since: Int?, _ notify: Bool) -> AsyncStream<CoreEvent> {
        lock.withLock { _calls.append((since, notify)) }
        return AsyncStream { $0.finish() }
    }
}

@MainActor
final class RunsTests: XCTestCase {
    private func defaults() -> UserDefaults { UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")! }

    private func makeRun(_ id: String = "r1", origin: String = "user", status: String = "done", title: String = "Summarise my notes",
                     ended: Bool = true, toolCalls: Int = 2) -> RunRecord {
        RunRecord(runID: id, sessionID: "s-\(id)", title: title, origin: origin, status: status,
                  started: Date(timeIntervalSince1970: 1_000), ended: ended ? Date(timeIntervalSince1970: 1_060) : nil, toolCalls: toolCalls)
    }

    private func approval(_ id: String = "a1", tool: String = "write_file", run runID: String? = "r1") -> CoreEvent {
        let request = ApprovalRequest(id: id, tool: tool, summary: "Save report.md", risk: "write", detail: "", alwaysRule: nil,
                                      canAlways: true, canSession: true, tainted: false, outsideSource: nil, reason: nil, sessionID: "s-r1")
        return CoreEvent(seq: 10, sessionID: "s-r1", runID: runID, kind: .approval(request))
    }

    private func runEnd(_ id: String = "r1") -> CoreEvent { CoreEvent(seq: 20, sessionID: "s-\(id)", runID: id, kind: .runEnd) }

    private func makeNotifier(runs: [RunRecord] = [], poster: RecordingPoster = RecordingPoster(), enabled: Bool = true,
                          frontmost: Bool = false) -> (RunNotifier, RecordingPoster) {
        let notifier = RunNotifier(poster: poster, runs: PreviewRunsClient(runs: runs), isEnabled: { enabled }, windowIsFrontmost: { frontmost })
        return (notifier, poster)
    }

    // MARK: No approve action

    func test_notificationHasNoApproveAction() async throws {
        let (notifier, poster) = makeNotifier(runs: [makeRun(status: "done")])
        await notifier.handle(approval())
        await notifier.handle(runEnd())
        XCTAssertEqual(poster.posted.count, 2)
        for notice in poster.posted {
            let words = (notice.title + " " + notice.body).lowercased()
            for forbidden in ["approve", "deny", "allow", "decline"] {
                XCTAssertFalse(words.contains(forbidden), "\(forbidden) in “\(words)”")
            }
        }
        // The real center is given a title, a body and nothing else: no category, so no action buttons.
        let request = SystemNotificationPoster.request(title: "Heron needs your OK: write_file", body: "Open Heron to answer.", identifier: "x")
        XCTAssertEqual(request.content.categoryIdentifier, "")
        XCTAssertTrue(request.content.userInfo.isEmpty)
        XCTAssertNil(request.trigger)
        XCTAssertEqual(request.content.title, "Heron needs your OK: write_file")
        XCTAssertEqual(request.content.body, "Open Heron to answer.")
    }

    func test_anApprovalNoticeNamesTheToolAndOpensTheApproval() async {
        let (notifier, poster) = makeNotifier()
        await notifier.handle(approval("a7", tool: "shell"))
        XCTAssertEqual(poster.posted.map(\.title), ["Heron needs your OK: shell"])
        XCTAssertEqual(RunNoticeTarget.parse(poster.posted[0].identifier), .approval(id: "a7"))
    }

    func test_anApprovalNoticeNeverHoldsTheApprovalsContent() async {
        let (notifier, poster) = makeNotifier()
        await notifier.handle(approval())
        let body = poster.posted.first?.body ?? ""
        XCTAssertFalse(body.contains("report.md"), "the summary and detail stay in the sheet: \(body)")
    }

    // MARK: Finished notices

    func test_noFinishedNoticeForScheduledProactiveOrTelegramRuns() async {
        for origin in ["schedule", "automation", "proactive", "telegram"] {
            let (notifier, poster) = makeNotifier(runs: [makeRun(origin: origin, status: "done")])
            await notifier.handle(runEnd())
            XCTAssertEqual(poster.posted, [], "a \(origin) run sends its own notice, or the owner is on the phone")
        }
        let (notifier, poster) = makeNotifier(runs: [makeRun(origin: "user", status: "done")])
        await notifier.handle(runEnd())
        XCTAssertEqual(poster.posted.map(\.title), ["Heron finished: Summarise my notes"], "an interactive run does")
    }

    func test_aFinishedNoticeOpensTheRun() async {
        let (notifier, poster) = makeNotifier(runs: [makeRun("r9")])
        await notifier.handle(runEnd("r9"))
        XCTAssertEqual(RunNoticeTarget.parse(poster.posted.first?.identifier ?? ""), .run(id: "r9"))
    }

    func test_everyEndedStatusOfAnInteractiveRunIsAnnouncedAsFinished() async {
        for status in ["done", "error", "cancelled"] {
            let (notifier, poster) = makeNotifier(runs: [makeRun(status: status)])
            await notifier.handle(runEnd())
            XCTAssertEqual(poster.posted.map(\.title), ["Heron finished: Summarise my notes"], status)
        }
    }

    func test_aRunTheCoreDoesNotListPostsNothing() async {
        let (notifier, poster) = makeNotifier(runs: [makeRun("other")])
        await notifier.handle(runEnd("r1"))
        XCTAssertEqual(poster.posted, [], "an unknown origin might be one that already notified")
    }

    func test_aRunListThatCantBeReadPostsNothing() async {
        let client = PreviewRunsClient(runs: [makeRun()])
        client.setFailure(.failed("down"))
        let poster = RecordingPoster()
        let notifier = RunNotifier(poster: poster, runs: client, isEnabled: { true }, windowIsFrontmost: { false })
        await notifier.handle(runEnd())
        XCTAssertEqual(poster.posted, [])
        client.setFailure(nil)
        await notifier.handle(runEnd())
        XCTAssertEqual(poster.posted.count, 1, "a later try can still post")
    }

    func test_aRunWithNoTitleIsStillNamed() async {
        let (notifier, poster) = makeNotifier(runs: [makeRun(title: "")])
        await notifier.handle(runEnd())
        XCTAssertEqual(poster.posted.map(\.title), ["Heron finished: your request"])
    }

    // MARK: When

    func test_nothingIsPostedForAnInteractiveRunWhileTheWindowIsFrontmost() async {
        let (notifier, poster) = makeNotifier(runs: [makeRun()], frontmost: true)
        await notifier.handle(approval())
        await notifier.handle(runEnd())
        XCTAssertEqual(poster.posted, [])
    }

    func test_aCoreNoticeIsPostedWhetherOrNotTheWindowIsFrontmost() async {
        for frontmost in [true, false] {
            let (notifier, poster) = makeNotifier(frontmost: frontmost)
            await notifier.handle(CoreEvent(seq: 3, kind: .notification(title: "Digest", body: "Three new items", source: "automation")))
            XCTAssertEqual(poster.posted, [.init(title: "Digest", body: "Three new items", identifier: "heron.notice.3")], "frontmost: \(frontmost)")
        }
    }

    func test_nothingIsPostedWhileTheSwitchIsOff() async {
        let (notifier, poster) = makeNotifier(runs: [makeRun()], enabled: false)
        await notifier.handle(approval())
        await notifier.handle(runEnd())
        await notifier.handle(CoreEvent(seq: 3, kind: .notification(title: "Digest", body: "x", source: "schedule")))
        XCTAssertEqual(poster.posted, [])
        XCTAssertEqual(poster.requests, 0)
    }

    func test_aRunsEndIsAnnouncedOnce() async {
        let (notifier, poster) = makeNotifier(runs: [makeRun()])
        await notifier.handle(runEnd())
        await notifier.handle(runEnd())
        XCTAssertEqual(poster.posted.count, 1)
    }

    func test_coreNoticesWithTheSameSeqAreBothPosted() async {
        // `seq` starts over when the core restarts, so a later notice can reuse an earlier number.
        let (notifier, poster) = makeNotifier()
        await notifier.handle(CoreEvent(seq: 3, kind: .notification(title: "One", body: "a", source: "schedule")))
        await notifier.handle(CoreEvent(seq: 3, kind: .notification(title: "Two", body: "b", source: "schedule")))
        XCTAssertEqual(poster.posted.map(\.title), ["One", "Two"])
    }

    func test_macOSIsAskedAtTheFirstNoticeAndNotBefore() async {
        let poster = RecordingPoster()
        poster.grants = false
        let (notifier, _) = makeNotifier(poster: poster)
        XCTAssertEqual(poster.requests, 0, "never at launch")
        await notifier.handle(approval())
        XCTAssertEqual(poster.requests, 1)
        XCTAssertEqual(poster.posted, [], "a refusal posts nothing")
    }

    // MARK: What a click opens

    func test_aNoticesIdentifierSaysWhatItOpens() {
        XCTAssertEqual(RunNoticeTarget.parse(RunNoticeTarget.approvalIdentifier("a-1")), .approval(id: "a-1"))
        XCTAssertEqual(RunNoticeTarget.parse(RunNoticeTarget.finishedIdentifier("r-1")), .run(id: "r-1"))
        XCTAssertEqual(RunNoticeTarget.parse("heron.notice.12"), RunNoticeTarget.none)
        XCTAssertEqual(RunNoticeTarget.parse(""), RunNoticeTarget.none)
    }

    func test_clickingAnApprovalNoticeOpensTheApprovalInTheInbox() async {
        let model = AppModel(client: PreviewCoreClient(), defaults: defaults(), poster: RecordingPoster())
        let request = ApprovalRequest(id: "a1", tool: "shell", summary: "Run it", risk: "exec", detail: "", alwaysRule: nil,
                                      canAlways: false, canSession: false, tainted: false, outsideSource: nil, reason: nil, sessionID: "other")
        model.approvals.add(request)
        await model.openFromNotification(identifier: RunNoticeTarget.approvalIdentifier("a1"))
        XCTAssertEqual(model.selection, .inbox)
        XCTAssertEqual(model.inbox.selection, .live("a1"))
    }

    func test_clickingAnApprovalNoticeThatWasAnsweredOpensTheRunList() async {
        let model = AppModel(client: PreviewCoreClient(), defaults: defaults(), poster: RecordingPoster())
        await model.openFromNotification(identifier: RunNoticeTarget.approvalIdentifier("gone"))
        XCTAssertEqual(model.selection, .runs)
    }

    func test_clickingAFinishedNoticeOpensTheRunsConversation() async throws {
        let client = PreviewCoreClient()
        let sessions = try await client.sessions()
        let sessionID = try XCTUnwrap(sessions.first?.id)
        let record = RunRecord(runID: "r5", sessionID: sessionID, title: "Plan", origin: "user", status: "done",
                               started: Date(timeIntervalSince1970: 1_000), ended: Date(timeIntervalSince1970: 1_060), toolCalls: 1)
        let model = AppModel(client: client, runs: PreviewRunsClient(runs: [record]), defaults: defaults(), poster: RecordingPoster())
        await model.openFromNotification(identifier: RunNoticeTarget.finishedIdentifier("r5"))
        XCTAssertEqual(model.selection, .chat)
        XCTAssertEqual(model.chat.currentSessionID, sessionID)
    }

    func test_clickingAFinishedNoticeForAnUnknownRunOpensTheRunList() async {
        let model = AppModel(client: PreviewCoreClient(), defaults: defaults(), poster: RecordingPoster())
        await model.openFromNotification(identifier: RunNoticeTarget.finishedIdentifier("nope"))
        XCTAssertEqual(model.selection, .runs)
    }

    // MARK: The store and the badge

    func test_theBadgeCountsTheRunsInWaiting() async {
        let client = PreviewRunsClient(runs: [
            makeRun("a", status: "waiting", ended: false), makeRun("b", status: "running", ended: false),
            makeRun("c", status: "waiting", ended: false), makeRun("d", status: "done"),
        ])
        let store = RunsStore(client: client, defaults: defaults())
        XCTAssertEqual(store.badgeCount, 0, "nothing loaded yet")
        await store.load()
        XCTAssertEqual(store.waitingCount, 2)
        XCTAssertEqual(store.badgeCount, 2)
        XCTAssertEqual(store.inProgress.map(\.runID), ["a", "b", "c"])
        XCTAssertEqual(store.finished.map(\.runID), ["d"])
    }

    func test_theBadgeIsGoneWhileTheSwitchIsOff() async {
        let d = defaults()
        d.set(false, forKey: RunsStore.defaultsKey)
        let store = RunsStore(client: PreviewRunsClient(runs: [makeRun(status: "waiting", ended: false)]), defaults: d)
        await store.load()
        XCTAssertFalse(store.enabled)
        XCTAssertEqual(store.waitingCount, 1)
        XCTAssertEqual(store.badgeCount, 0)
    }

    func test_waitingReturnsToRunning() async {
        let store = RunsStore(client: PreviewRunsClient(runs: [makeRun(status: "running", ended: false)]), defaults: defaults())
        await store.load()
        store.handle(approval())
        XCTAssertEqual(store.runs.first?.status, "waiting")
        XCTAssertEqual(store.badgeCount, 1)
        store.handle(CoreEvent(seq: 11, sessionID: "s-r1", runID: "r1", kind: .approvalResolved(id: "a1", decision: "allow")))
        XCTAssertEqual(store.runs.first?.status, "running")
        XCTAssertEqual(store.badgeCount, 0)
        store.handle(approval())
        store.handle(CoreEvent(seq: 12, sessionID: "s-r1", runID: "r1", kind: .approvalParked(id: "a1", parkedID: 4)))
        XCTAssertEqual(store.runs.first?.status, "running")
    }

    func test_aFinishedRunIsNotReopenedByALateApproval() async {
        let store = RunsStore(client: PreviewRunsClient(runs: [makeRun(status: "done")]), defaults: defaults())
        await store.load()
        store.handle(approval())
        XCTAssertEqual(store.runs.first?.status, "done")
    }

    func test_theListIsWhatTheCoreSays() async {
        let client = PreviewRunsClient(runs: [makeRun("a", status: "running", ended: false)])
        let store = RunsStore(client: client, defaults: defaults())
        await store.load()
        XCTAssertEqual(store.loadState, .loaded)
        client.setRuns([makeRun("b"), makeRun("a")])
        await store.load()
        XCTAssertEqual(store.runs.map(\.runID), ["b", "a"])
        client.setFailure(.failed("down"))
        await store.load()
        XCTAssertEqual(store.loadState, .failed("down"))
        XCTAssertEqual(store.runs.map(\.runID), ["b", "a"], "a failed reload keeps what was shown")
    }

    func test_rowsAreDescribedInPlainWords() {
        XCTAssertEqual(RunsStore.originPhrase("user"), "You asked")
        XCTAssertEqual(RunsStore.originPhrase("telegram"), "From Telegram")
        XCTAssertEqual(RunsStore.statusLabel("waiting"), "Waiting")
        let now = Date(timeIntervalSince1970: 1_070)
        XCTAssertEqual(RunsStore.detailLine(makeRun(toolCalls: 1), now: now), "You asked · 1 tool call · just now")
        XCTAssertTrue(RunsStore.detailLine(makeRun(origin: "schedule", toolCalls: 3), now: now).hasPrefix("A schedule · 3 tool calls · "))
    }

    // MARK: The switch

    func test_theRowIsOnByDefaultAndAsksForNotifications() throws {
        let row = try XCTUnwrap(FeatureRows.all.first { $0.key == "runs" })
        XCTAssertEqual(row.title, "Runs and notifications")
        XCTAssertEqual(row.kind, .app(defaultsKey: RunsSetting.defaultsKey))
        XCTAssertEqual(row.permission, FeatureRow.notificationsPermission)
        XCTAssertNotNil(row.settingsPane)
        let d = defaults()
        _ = RunsStore(client: PreviewRunsClient(), defaults: d)
        let store = FeaturesSettingsStore(client: PreviewFeaturesClient(), rows: FeatureRows.all, defaults: d, poster: RecordingPoster())
        XCTAssertEqual(store.items.first { $0.row.key == "runs" }?.on, true)
    }

    func test_turningTheRowOffTurnsTheStoreOff() async throws {
        let d = defaults()
        let runs = RunsStore(client: PreviewRunsClient(), defaults: d)
        let store = FeaturesSettingsStore(client: PreviewFeaturesClient(), rows: FeatureRows.all, defaults: d, poster: RecordingPoster())
        let row = try XCTUnwrap(FeatureRows.all.first { $0.key == "runs" })
        await store.set(row, on: false)
        XCTAssertTrue(runs.refreshEnabled())
        XCTAssertFalse(runs.enabled)
        XCTAssertFalse(runs.refreshEnabled(), "no change the second time")
    }

    func test_theStreamOpensWithNotifyWhenTheSettingIsOn() {
        let recorder = OpenRecorder()
        let d = defaults()
        let model = AppModel(client: PreviewCoreClient(), defaults: d, poster: RecordingPoster(), openEvents: recorder.open)
        model.startEvents()
        XCTAssertEqual(recorder.calls.map { $0.notify }, [true])
        model.stopEvents()
    }

    func test_theStreamOpensWithoutNotifyWhenTheSettingIsOff() {
        let recorder = OpenRecorder()
        let d = defaults()
        d.set(false, forKey: RunsStore.defaultsKey)
        let model = AppModel(client: PreviewCoreClient(), defaults: d, poster: RecordingPoster(), openEvents: recorder.open)
        model.startEvents()
        XCTAssertEqual(recorder.calls.map { $0.notify }, [false], "the core keeps posting its own notices")
        model.stopEvents()
    }

    func test_theStreamIsReopenedWhenTheSettingChanges() {
        let recorder = OpenRecorder()
        let d = defaults()
        let model = AppModel(client: PreviewCoreClient(), defaults: d, poster: RecordingPoster(), openEvents: recorder.open)
        model.startEvents()
        d.set(false, forKey: RunsStore.defaultsKey)
        model.applyRunsSetting()
        d.set(true, forKey: RunsStore.defaultsKey)
        model.applyRunsSetting()
        model.applyRunsSetting()
        XCTAssertEqual(recorder.calls.map { $0.notify }, [true, false, true], "one reopening per change")
        model.stopEvents()
    }

    func test_theRunsScreenLeavesWhenTheSettingIsTurnedOff() {
        let d = defaults()
        let model = AppModel(client: PreviewCoreClient(), defaults: d, poster: RecordingPoster())
        model.selection = .runs
        d.set(false, forKey: RunsStore.defaultsKey)
        model.applyRunsSetting()
        XCTAssertEqual(model.selection, .today)
    }

    func test_aRoutedApprovalReachesTheNotifier() async throws {
        let poster = RecordingPoster()
        let model = AppModel(client: PreviewCoreClient(), defaults: defaults(), poster: poster, windowIsFrontmost: { false })
        model.route(approval("a3", tool: "send_email"))
        for _ in 0..<100 where poster.posted.isEmpty { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertEqual(poster.posted.map(\.title), ["Heron needs your OK: send_email"])
    }

    // MARK: Where it shows

    func test_runsIsAScreenOfItsOwnAfterTheSeven() {
        XCTAssertFalse(Destination.allCases.contains(.runs), "the sidebar adds it only while the switch is on")
        XCTAssertEqual(Destination.allCases.map(\.shortcutDigit), [1, 2, 3, 4, 5, 6, 7])
        XCTAssertEqual(Destination.runs.shortcutDigit, 8)
        XCTAssertEqual(Destination.runs.title, "Runs")
    }

    func test_theMenuBarIconGrowsToShowTheBadge() {
        XCTAssertEqual(MenuBarIcon.image(waiting: false).size.width, 18)
        XCTAssertGreaterThan(MenuBarIcon.image(waiting: true, badge: 3).size.width, 18)
        XCTAssertEqual(MenuBarIcon.image(waiting: true, badge: 1).accessibilityDescription, "Heron, 1 run waiting for you")
        XCTAssertEqual(MenuBarIcon.image(waiting: true, badge: 2).accessibilityDescription, "Heron, 2 runs waiting for you")
        XCTAssertEqual(MenuBarIcon.image(waiting: true).accessibilityDescription, "Heron, waiting for you")
    }
}
