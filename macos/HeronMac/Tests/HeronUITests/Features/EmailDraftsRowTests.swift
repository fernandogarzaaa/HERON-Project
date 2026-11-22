import XCTest
@testable import HeronUI

final class EmailDraftsRowTests: XCTestCase {
    private var row: FeatureRow? { FeatureRows.all.first { $0.key == "email_drafts" } }

    func testEmailDraftsRowIsACoreKeyRow() throws {
        let row = try XCTUnwrap(row)
        XCTAssertEqual(row.title, "Email drafts")
        XCTAssertEqual(row.kind, .core(key: "email_drafts"))
    }

    func testEmailDraftsRowNeedsNoMacOSPermission() throws {
        let row = try XCTUnwrap(row)
        XCTAssertNil(row.permission)
        XCTAssertNil(row.settingsPane)
    }

    func testEmailDraftsRowAppearsOnce() {
        XCTAssertEqual(FeatureRows.all.filter { $0.key == "email_drafts" }.count, 1)
    }
}
