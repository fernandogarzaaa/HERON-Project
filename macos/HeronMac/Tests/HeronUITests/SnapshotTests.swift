import XCTest
import SwiftUI
import HeronCore
@testable import HeronUI

/// Snapshots of the two screens the visual doc specifies exactly: the approval sheet and the risk notice
/// (heron-mac-visual.md, Verification), in light and dark, Dusk theme, `system` accent drawn as the default blue.
@MainActor
final class SnapshotTests: XCTestCase {
    private func request(tainted: Bool = false, scopes: Bool = true, high: Bool = false) -> ApprovalRequest {
        if high {
            return ApprovalRequest(id: "h1", tool: "send_email", summary: "Send the Friday summary", risk: "external",
                                   detail: "To: team@northwind.example\nSubject: Friday summary", alwaysRule: nil, canAlways: false, canSession: false,
                                   tainted: false, outsideSource: nil, reason: "Heron wants to send the weekly summary you asked for.", sessionID: "s1")
        }
        return ApprovalRequest(id: "a1", tool: "write_file", summary: "Create a file in your working folder", risk: "write",
                               detail: "~/Work/drafts/northwind-reply.md", alwaysRule: scopes ? "write_file:~/Work/drafts/**" : nil,
                               canAlways: scopes, canSession: scopes, tainted: tainted,
                               outsideSource: tainted ? "an email from billing@northwind.example" : nil,
                               reason: "Heron wants to save the reply draft for Northwind.", sessionID: "s1")
    }

    private let sheetSize = CGSize(width: 520, height: 0)
    private let screenSize = CGSize(width: 1180, height: 760)

    // MARK: Approval sheet

    func testApprovalSheetCleanConversationOffersAllThreeScopesLight() throws {
        try assertSnapshot(ApprovalSheetView(request: request()) { _ in }, name: "approval-clean-light", size: sheetSize, dark: false)
    }

    func testApprovalSheetCleanConversationOffersAllThreeScopesDark() throws {
        try assertSnapshot(ApprovalSheetView(request: request()) { _ in }, name: "approval-clean-dark", size: sheetSize, dark: true)
    }

    func testApprovalSheetTaintedConversationSavesNothingLight() throws {
        try assertSnapshot(ApprovalSheetView(request: request(tainted: true, scopes: false)) { _ in },
                           name: "approval-tainted-light", size: sheetSize, dark: false)
    }

    func testApprovalSheetTaintedConversationSavesNothingDark() throws {
        try assertSnapshot(ApprovalSheetView(request: request(tainted: true, scopes: false)) { _ in },
                           name: "approval-tainted-dark", size: sheetSize, dark: true)
    }

    func testApprovalSheetHighRiskNamesTheActionLight() throws {
        try assertSnapshot(ApprovalSheetView(request: request(high: true)) { _ in }, name: "approval-high-risk-light", size: sheetSize, dark: false)
    }

    // MARK: Risk notice

    private func notice() async throws -> RiskNotice {
        try await PreviewCoreClient().riskNotice()
    }

    func testRiskNoticeWithSandboxOnLight() async throws {
        let n = try await notice()
        try assertSnapshot(RiskNoticeScreen(notice: n, sandboxOn: true, accepted: .constant(false)),
                           name: "risk-notice-sandbox-on-light", size: screenSize, dark: false)
    }

    func testRiskNoticeWithSandboxOnDark() async throws {
        let n = try await notice()
        try assertSnapshot(RiskNoticeScreen(notice: n, sandboxOn: true, accepted: .constant(false)),
                           name: "risk-notice-sandbox-on-dark", size: screenSize, dark: true)
    }

    func testRiskNoticeWithSandboxOffDark() async throws {
        let n = try await notice()
        try assertSnapshot(RiskNoticeScreen(notice: n, sandboxOn: false, accepted: .constant(true)),
                           name: "risk-notice-sandbox-off-dark", size: screenSize, dark: true)
    }
}

/// The comparison logic itself: identical pictures match, changed ones don't, the 1% tolerance holds.
@MainActor
final class SnapshotComparisonTests: XCTestCase {
    private func render(_ color: Color, width: CGFloat = 100, height: CGFloat = 100, corner: Color? = nil, cornerSize: CGFloat = 0) throws -> CGImage {
        let view = ZStack(alignment: .topLeading) {
            color
            if let corner { corner.frame(width: cornerSize, height: cornerSize) }
        }.frame(width: width, height: height)
        return try Snapshot.render(view, width: width, height: height)
    }

    func testIdenticalImagesHaveNoDifference() throws {
        let a = try render(.red), b = try render(.red)
        let d = Snapshot.difference(a, b)
        XCTAssertEqual(d.ratio, 0)
        XCTAssertFalse(d.sizeChanged)
    }

    func testDifferentColoursDifferEverywhere() throws {
        let d = Snapshot.difference(try render(.red), try render(.blue))
        XCTAssertEqual(d.ratio, 1, accuracy: 0.0001)
    }

    func testDifferentSizesCountAsFullyDifferent() throws {
        let d = Snapshot.difference(try render(.red, width: 100), try render(.red, width: 90))
        XCTAssertEqual(d.ratio, 1)
        XCTAssertTrue(d.sizeChanged)
    }

    func testSmallChangeStaysInsideTheTolerance() throws {
        // A 9 x 9 patch is 0.81% of a 100 x 100 picture; a 12 x 12 patch is 1.44%.
        let base = try render(.red)
        let small = try render(.red, corner: .blue, cornerSize: 9)
        let big = try render(.red, corner: .blue, cornerSize: 12)
        XCTAssertEqual(Snapshot.difference(base, small).ratio, 0.0081, accuracy: 0.0005)
        XCTAssertLessThanOrEqual(Snapshot.difference(base, small).ratio, Snapshot.pixelTolerance)
        XCTAssertGreaterThan(Snapshot.difference(base, big).ratio, Snapshot.pixelTolerance)
    }

    func testChannelNoiseBelowTheToleranceIsIgnored() throws {
        let a = try render(Color(.sRGB, red: 0.5, green: 0.5, blue: 0.5))
        let nearby = try render(Color(.sRGB, red: 0.5 + 6.0 / 255, green: 0.5, blue: 0.5))
        let far = try render(Color(.sRGB, red: 0.5 + 40.0 / 255, green: 0.5, blue: 0.5))
        XCTAssertEqual(Snapshot.difference(a, nearby).ratio, 0)
        XCTAssertEqual(Snapshot.difference(a, far).ratio, 1, accuracy: 0.0001)
    }

    func testMissingReferenceIsRecordedThenCompared() throws {
        let name = "zz-unit-\(UUID().uuidString)"
        defer { try? FileManager.default.removeItem(at: Snapshot.referenceURL(name)) }
        let image = try render(.green)
        guard case .recorded(let url) = try Snapshot.compare(image, name: name) else { return XCTFail("expected a recording") }
        XCTAssertTrue(url.lastPathComponent.hasSuffix(".macos\(Snapshot.macOSMajor).png"))
        XCTAssertEqual(try Snapshot.compare(image, name: name), .matched(difference: 0))
        guard case .mismatch(let d, let sizeChanged) = try Snapshot.compare(try render(.blue), name: name) else { return XCTFail("expected a mismatch") }
        XCTAssertEqual(d, 1, accuracy: 0.0001)
        XCTAssertFalse(sizeChanged)
    }

    func testReferenceFromAnotherMacOSIsSkipped() throws {
        let name = "zz-unit-\(UUID().uuidString)"
        let other = Snapshot.referenceURL(name, major: Snapshot.macOSMajor + 1)
        try FileManager.default.createDirectory(at: Snapshot.directory, withIntermediateDirectories: true)
        try Snapshot.pngData(try render(.green)).write(to: other)
        defer { try? FileManager.default.removeItem(at: other) }
        XCTAssertEqual(try Snapshot.compare(try render(.green), name: name), .skippedOtherMacOS)
        XCTAssertFalse(FileManager.default.fileExists(atPath: Snapshot.referenceURL(name).path), "nothing is recorded over another version's reference")
    }
}
