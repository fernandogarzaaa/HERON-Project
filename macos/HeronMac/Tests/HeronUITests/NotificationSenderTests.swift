import XCTest
@testable import HeronUI

final class NotificationSenderTests: XCTestCase {
    private struct Failure: LocalizedError {
        var errorDescription: String? { "the center said no" }
    }

    /// A `NotificationCenterClient` that answers as told and records what it was asked.
    private final class FakeCenter: NotificationCenterClient, @unchecked Sendable {
        private let lock = NSLock()
        private var _status: NotificationAuthorization
        private let grants: Bool
        private let requestFails: Bool
        private let addFails: Bool
        private var _requests = 0
        private var _added: [(identifier: String, title: String, body: String)] = []

        init(_ status: NotificationAuthorization, grants: Bool = true, requestFails: Bool = false, addFails: Bool = false) {
            _status = status
            self.grants = grants
            self.requestFails = requestFails
            self.addFails = addFails
        }

        var requests: Int { lock.withLock { _requests } }
        var added: [String] { lock.withLock { _added.map { "\($0.identifier)|\($0.title)|\($0.body)" } } }

        func authorization() async -> NotificationAuthorization { lock.withLock { _status } }

        func requestAuthorization() async throws -> Bool {
            lock.withLock { _requests += 1 }
            if requestFails { throw Failure() }
            lock.withLock { _status = grants ? .allowed : .denied }
            return grants
        }

        func add(identifier: String, title: String, body: String) async throws {
            if addFails { throw Failure() }
            lock.withLock { _added.append((identifier, title, body)) }
        }
    }

    private final class Log: @unchecked Sendable {
        private let lock = NSLock()
        private var _lines: [String] = []
        var lines: [String] { lock.withLock { _lines } }
        func append(_ line: String) { lock.withLock { _lines.append(line) } }
    }

    private func send(_ center: FakeCenter, _ log: Log) async -> NotificationSender.Outcome {
        await NotificationSender(center: center, log: { log.append($0) }).send(title: "Digest", body: "All done.", identifier: "heron.notification.0.3")
    }

    func test_allowedPostsWithoutAskingAgain() async {
        let center = FakeCenter(.allowed)
        let log = Log()
        let outcome = await send(center, log)
        XCTAssertEqual(outcome, .posted)
        XCTAssertEqual(center.added, ["heron.notification.0.3|Digest|All done."])
        XCTAssertEqual(center.requests, 0)
        XCTAssertEqual(log.lines, [])
    }

    func test_theFirstNotificationAsksThenPosts() async {
        let center = FakeCenter(.notDetermined)
        let log = Log()
        let first = await send(center, log)
        let second = await send(center, log)
        XCTAssertEqual([first, second], [.posted, .posted])
        XCTAssertEqual(center.requests, 1, "the prompt is shown once")
        XCTAssertEqual(center.added.count, 2)
        XCTAssertEqual(log.lines, [])
    }

    func test_declinedAtThePromptPostsNothing() async {
        let center = FakeCenter(.notDetermined, grants: false)
        let log = Log()
        let outcome = await send(center, log)
        XCTAssertEqual(outcome, .notAllowed)
        XCTAssertEqual(center.added, [])
        XCTAssertEqual(log.lines, [], "declining is the owner's choice, not a failure")
    }

    func test_deniedEarlierNeitherAsksNorPosts() async {
        let center = FakeCenter(.denied)
        let log = Log()
        let outcome = await send(center, log)
        XCTAssertEqual(outcome, .notAllowed)
        XCTAssertEqual(center.requests, 0)
        XCTAssertEqual(center.added, [])
    }

    func test_aFailedPermissionRequestIsLogged() async {
        let center = FakeCenter(.notDetermined, requestFails: true)
        let log = Log()
        let outcome = await send(center, log)
        XCTAssertEqual(outcome, .notAllowed)
        XCTAssertEqual(center.added, [])
        XCTAssertEqual(log.lines, ["Asking for notification permission failed: the center said no"])
    }

    func test_aFailedPostIsLoggedWithItsIdentifierButNotItsText() async {
        let center = FakeCenter(.allowed, addFails: true)
        let log = Log()
        let outcome = await send(center, log)
        XCTAssertEqual(outcome, .failed)
        XCTAssertEqual(log.lines, ["Posting notification heron.notification.0.3 failed: the center said no"])
        XCTAssertFalse(log.lines.joined().contains("All done."), "the notification's text stays out of the system log")
    }
}
