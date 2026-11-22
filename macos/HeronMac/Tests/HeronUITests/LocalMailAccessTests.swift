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

    func testAnAppleEventsListBecomesMessagesAndTheNoAccountsAnswerIsItsOwnProblem() throws {
        let date = Date(timeIntervalSince1970: 1_799_999_000)
        func row(_ id: String, account: String) -> NSAppleEventDescriptor {
            let r = NSAppleEventDescriptor.list()
            for (i, v) in [id, "Dana <d@example.com>", "Invoice"].enumerated() { r.insert(NSAppleEventDescriptor(string: v), at: i + 1) }
            r.insert(NSAppleEventDescriptor(date: date), at: 4)
            r.insert(NSAppleEventDescriptor(string: account), at: 5)
            r.insert(NSAppleEventDescriptor(string: "Please pay."), at: 6)
            return r
        }
        let list = NSAppleEventDescriptor.list()
        list.insert(row("<a@x>", account: "iCloud"), at: 1)
        list.insert(row("<a@x>", account: "Outlook"), at: 2)
        let parsed = try SystemMailAccess.parse(list)
        XCTAssertEqual(parsed.map(\.id), ["<a@x>", "<a@x>"])
        XCTAssertEqual(parsed[0].date, 1_799_999_000)
        XCTAssertEqual([parsed[0].from, parsed[0].subject, parsed[0].body], ["Dana <d@example.com>", "Invoice", "Please pay."])
        XCTAssertEqual(try SystemMailAccess.parse(NSAppleEventDescriptor.list()), [])
        XCTAssertThrowsError(try SystemMailAccess.parse(NSAppleEventDescriptor(string: "no-accounts"))) { XCTAssertEqual($0 as? MailAccessError, .noAccounts) }
        XCTAssertThrowsError(try SystemMailAccess.parse(NSAppleEventDescriptor(string: "surprise"))) { XCTAssertEqual($0 as? MailAccessError, .failed("Mail answered in a way Heron didn't expect.")) }
    }
}
