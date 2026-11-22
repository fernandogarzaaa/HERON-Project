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

    func testTheRealPathRunsOsascriptAsAChildNeverNSAppleScript() throws {
        // `run` passes the script on stdin to /usr/bin/osascript. Here only a script that touches no app, to check the plumbing
        // (stdin in, text out, stderr and the exit status on a failure) without Mail or any Automation prompt.
        XCTAssertEqual(try SystemMailAccess.run("return \"ok\"").trimmingCharacters(in: .whitespacesAndNewlines), "ok")
        XCTAssertThrowsError(try SystemMailAccess.run("error \"Not authorized\" number -1743")) { XCTAssertEqual($0 as? MailAccessError, .notAuthorized) }
    }
}
