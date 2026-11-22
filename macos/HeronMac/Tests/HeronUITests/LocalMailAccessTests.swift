import XCTest
import Foundation
import HeronCore
@testable import HeronUI

/// The parts of reading Mail that need no Mail: what the app keeps of what it reads, and how Apple Events answers are read.
final class LocalMailAccessTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_800_000_000)
    private func message(_ id: String, minutesAgo: Int = 1, account: String = "iCloud", body: String = "hi") -> LocalMailMessage {
        LocalMailMessage(id: id, from: "a@example.com", subject: "S \(id)", date: Int(now.timeIntervalSince1970) - minutesAgo * 60, account: account, body: body)
    }

    func testTheSameMessageIdInTwoAccountsIsOneMessageNamingBoth() {
        let kept = LocalMailPolicy.prepare([message("<d@x>", account: "Gmail"), message("<d@x>", account: "iCloud"), message("<d@x>", account: "Gmail")], now: now)
        XCTAssertEqual(kept.count, 1)
        XCTAssertEqual(kept[0].account, "Gmail, iCloud")
    }

    func testOnlyTheNewestThreeHundredAreKeptNewestFirst() {
        let many = (0..<350).map { message("<m\($0)@x>", minutesAgo: $0 + 1) }
        let kept = LocalMailPolicy.prepare(many.shuffled(), now: now)
        XCTAssertEqual(kept.count, LocalMailPolicy.maxMessages)
        XCTAssertEqual(kept.first?.id, "<m0@x>")
        XCTAssertEqual(kept.last?.id, "<m299@x>")
        XCTAssertEqual(kept.map(\.date), kept.map(\.date).sorted(by: >))
    }

    func testMessagesOlderThanFourteenDaysOrWithoutAnIdAreDropped() {
        let old = message("<old@x>", minutesAgo: 14 * 24 * 60 + 5)
        let edge = message("<edge@x>", minutesAgo: 14 * 24 * 60 - 5)
        let kept = LocalMailPolicy.prepare([old, edge, message("  "), message("")], now: now)
        XCTAssertEqual(kept.map(\.id), ["<edge@x>"])
    }

    func testLongBodiesAreTruncated() {
        let kept = LocalMailPolicy.prepare([message("<l@x>", body: String(repeating: "b", count: 60_000))], now: now)
        XCTAssertEqual(kept[0].body.count, LocalMailPolicy.maxBodyCharacters)
    }

    func testNoMessagesIsAnEmptyListNotAnError() {
        XCTAssertEqual(LocalMailPolicy.prepare([], now: now), [])
    }

    func testAppleEventErrorsBecomeTheRightProblem() {
        XCTAssertEqual(SystemMailAccess.map(errorNumber: -1743, message: "x"), .notAuthorized)
        XCTAssertEqual(SystemMailAccess.map(errorNumber: -600, message: "x"), .notRunning)
        XCTAssertEqual(SystemMailAccess.map(errorNumber: -609, message: "x"), .notRunning)
        XCTAssertEqual(SystemMailAccess.map(errorNumber: 5, message: "Boom"), .failed("Boom"))
        XCTAssertEqual(SystemMailAccess.map(errorNumber: 5, message: ""), .failed("Mail answered with error 5."))
    }

    func testTheScriptAsksForTheLastFourteenDaysUpToTheCapAndNeverTouchesTheMailFolder() {
        let script = SystemMailAccess.script(daysBack: 14, limit: 300, bodyCharacters: 4000)
        XCTAssertTrue(script.contains("14 * days") && script.contains("300") && script.contains("4000"))
        XCTAssertTrue(script.contains("tell application \"Mail\""))
        XCTAssertFalse(script.contains("Library/Mail"), "no reading of Mail's own files")
        XCTAssertTrue(script.contains("messages of inbox whose"), "Mail's own unified inbox, not a mailbox looked up by name")
        XCTAssertFalse(script.contains("mailbox \"INBOX\""), "a mailbox name can miss (Gmail, Exchange, localized names)")
        XCTAssertTrue(script.contains("timeout"), "the wait for Mail to open is allowed for")
    }

    func testTheScriptsPrintedTextBecomesMessagesAndNoAccountsIsItsOwnProblem() throws {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        let rs = String(SystemMailAccess.recordSeparator), us = String(SystemMailAccess.fieldSeparator)
        func record(_ id: String, account: String, body: String = "Please pay.\nThanks") -> String {
            [id, "Dana <d@example.com>", "Invoice: March", "1000", account, body].joined(separator: us)
        }
        let output = record("<a@x>", account: "iCloud") + rs + record("<a@x>", account: "Outlook", body: "") + rs + "\n"
        let parsed = try SystemMailAccess.parse(output, now: now)
        XCTAssertEqual(parsed.map(\.id), ["<a@x>", "<a@x>"])
        XCTAssertEqual(parsed[0].date, 1_800_000_000 - 1000)
        XCTAssertEqual([parsed[0].from, parsed[0].subject, parsed[0].account], ["Dana <d@example.com>", "Invoice: March", "iCloud"])
        XCTAssertEqual(parsed[0].body, "Please pay.\nThanks", "newlines in the body survive")
        XCTAssertEqual(parsed[1].body, "", "an empty body is still a message")
        XCTAssertEqual(try SystemMailAccess.parse("", now: now), [], "no messages in the window is an empty list")
        XCTAssertEqual(try SystemMailAccess.parse("\n", now: now), [])
        XCTAssertThrowsError(try SystemMailAccess.parse("no-accounts\n", now: now)) { XCTAssertEqual($0 as? MailAccessError, .noAccounts) }
        // A damaged record is skipped, not guessed at.
        XCTAssertEqual(try SystemMailAccess.parse("only" + us + "three" + us + "fields" + rs + record("<b@x>", account: "Gmail"), now: now).map(\.id), ["<b@x>"])
    }

    func testOsascriptsStderrBecomesTheRightProblem() {
        XCTAssertEqual(SystemMailAccess.failure(stderr: "execution error: Not authorized to send Apple events to Mail. (-1743)\n", status: 1), .notAuthorized)
        XCTAssertEqual(SystemMailAccess.failure(stderr: "execution error: Application isn't running. (-600)", status: 1), .notRunning)
        XCTAssertEqual(SystemMailAccess.failure(stderr: "execution error: Something odd happened. (-1728)", status: 1), .failed("Something odd happened. (-1728)"))
        XCTAssertEqual(SystemMailAccess.failure(stderr: "", status: 15), .failed("Mail didn't answer in time. Try again."))
        XCTAssertEqual(SystemMailAccess.failure(stderr: "", status: 1), .failed("Mail answered with error 0."))
    }

    // MARK: A Mail that was only just launched

    private final class Calls: @unchecked Sendable {
        private let lock = NSLock()
        private var runs = 0
        private var naps: [Duration] = []
        var runCount: Int { lock.withLock { runs } }
        var sleeps: [Duration] { lock.withLock { naps } }
        func ran() -> Int { lock.withLock { runs += 1; return runs } }
        func slept(_ d: Duration) { lock.withLock { naps.append(d) } }
    }

    private var record: String {
        ["<a@x>", "Dana", "Hi", "60", "iCloud", "Text"].joined(separator: String(SystemMailAccess.fieldSeparator)) + String(SystemMailAccess.recordSeparator)
    }

    private func access(running: Bool, answers: [String], calls: Calls) -> SystemMailAccess {
        SystemMailAccess(runner: { _ in
            let n = calls.ran()
            return answers[min(n, answers.count) - 1]
        }, isRunning: { running }, sleep: { calls.slept($0) }, launchGrace: .seconds(5))
    }

    func testAnEmptyAnswerFromAMailThatWasJustLaunchedWaitsAndAsksOnce() async throws {
        let calls = Calls()
        let mail = access(running: false, answers: ["", record], calls: calls)
        let messages = try await mail.messages()
        XCTAssertEqual(messages.map(\.id), ["<a@x>"], "the second answer, after Mail loaded, is used")
        XCTAssertEqual(calls.runCount, 2)
        XCTAssertEqual(calls.sleeps, [.seconds(5)])
    }

    func testStillEmptyAfterTheOneRetryIsEmptyNotAnError() async throws {
        let calls = Calls()
        let messages = try await access(running: false, answers: ["", ""], calls: calls).messages()
        XCTAssertEqual(messages, [])
        XCTAssertEqual(calls.runCount, 2, "asked once more, not in a loop")
    }

    func testNoRetryWhenMailWasAlreadyRunningOrAnswered() async throws {
        let running = Calls()
        _ = try await access(running: true, answers: [""], calls: running).messages()
        XCTAssertEqual(running.runCount, 1)
        XCTAssertEqual(running.sleeps, [])
        let answered = Calls()
        _ = try await access(running: false, answers: [record], calls: answered).messages()
        XCTAssertEqual(answered.runCount, 1)
        XCTAssertEqual(answered.sleeps, [])
    }

    func testNoAccountsAndErrorsAreNotRetried() async throws {
        let calls = Calls()
        do { _ = try await access(running: false, answers: ["no-accounts"], calls: calls).messages(); XCTFail("expected no accounts") }
        catch { XCTAssertEqual(error as? MailAccessError, .noAccounts) }
        XCTAssertEqual(calls.runCount, 1)
        XCTAssertEqual(calls.sleeps, [])
        let failing = SystemMailAccess(runner: { _ in throw MailAccessError.notAuthorized }, isRunning: { false }, sleep: { _ in }, launchGrace: .zero)
        do { _ = try await failing.messages(); XCTFail("expected a refusal") }
        catch { XCTAssertEqual(error as? MailAccessError, .notAuthorized) }
    }

    func testTheRealPathRunsOsascriptAsAChildNeverNSAppleScript() throws {
        // `run` passes the script on stdin to /usr/bin/osascript. Here only a script that touches no app, to check the plumbing
        // (stdin in, text out, stderr and the exit status on a failure) without Mail or any Automation prompt.
        XCTAssertEqual(try SystemMailAccess.run("return \"ok\"").trimmingCharacters(in: .whitespacesAndNewlines), "ok")
        XCTAssertThrowsError(try SystemMailAccess.run("error \"Not authorized\" number -1743")) { XCTAssertEqual($0 as? MailAccessError, .notAuthorized) }
    }
}
