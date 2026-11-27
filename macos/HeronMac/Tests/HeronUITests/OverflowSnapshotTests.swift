import XCTest
import SwiftUI
import AppKit
import HeronCore
@testable import HeronUI

/// Overflow at the smallest window the app allows (heron-v1.md item 41b): long content scrolls in one place,
/// and the footer and buttons stay visible. Light mode, Dusk theme, long demo content from `PreviewDemo`.
///
/// These render through an `NSHostingView` (`Snapshot.checkScrolling`), because `ImageRenderer` can't draw
/// scroll views. In each picture, content may be cut off only at a scroll edge.
@MainActor
final class OverflowSnapshotTests: XCTestCase {
    private let firstRunSize = WindowMinimum.firstRun
    private let pageSize = WindowMinimum.page
    private let windowSize = WindowMinimum.main

    // MARK: Chat

    private func chatModel(seed: (PreviewCoreClient) -> String) async -> AppModel {
        let client = PreviewCoreClient()
        let id = seed(client)
        let defaults = UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: defaults), userFirstName: "Sam")
        await model.chat.open(id: id)
        return model
    }

    func testLongTranscriptScrollsAndTheComposerStaysVisibleLight() async throws {
        let model = await chatModel { $0.seedLongConversation(); return "s-long" }
        XCTAssertEqual(model.chat.messages.count, 16, "the long demo conversation loaded")
        try assertScrollingSnapshot(ChatView(model: model), name: "overflow-chat-transcript-light", size: pageSize)
    }

    func testLongToolOutputScrollsInTheTranscriptLight() async throws {
        let model = await chatModel { $0.seedLongToolOutputConversation(); return "s-tool" }
        XCTAssertEqual(model.chat.messages.last?.steps.first?.sentence, PreviewDemo.longToolOutput)
        try assertScrollingSnapshot(ChatView(model: model, expandSteps: true), name: "overflow-chat-tool-output-light", size: pageSize)
    }

    /// A conversation whose reply the core restart cut off, with a new prompt typed: the box keeps its one-line
    /// height, Send (not Stop) shows, and the transcript keeps the rest of the page.
    func testAnUnfinishedConversationTakesANewPromptLight() async throws {
        let client = PreviewCoreClient()
        try await client.acceptRiskNotice(version: 3)
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!), userFirstName: "Sam")
        let stream = client.events(since: nil)
        await model.chat.send("Draft a reply to Maria")
        for await event in stream { model.chat.handle(event); if case .approval = event.kind { break } }
        client.simulateCoreRestart()
        model.chat.coreWentAway()
        model.chat.composer = "Try that again"
        XCTAssertTrue(model.chat.canSend)
        try assertScrollingSnapshot(ChatView(model: model), name: "chat-unfinished-new-prompt-light", size: pageSize)
    }

    // MARK: Risk notice

    func testLongRiskListScrollsInsideTheBoxAndTheFooterStaysVisibleLight() throws {
        try assertScrollingSnapshot(RiskNoticeScreen(notice: PreviewDemo.longRiskNotice, sandboxOn: true, accepted: .constant(false)),
                                    name: "overflow-risk-notice-long-light", size: firstRunSize)
    }

    func testShortRiskListBoxIsAsTallAsItsTextLight() throws {
        try assertScrollingSnapshot(RiskNoticeScreen(notice: PreviewDemo.shortRiskNotice, sandboxOn: true, accepted: .constant(false)),
                                    name: "overflow-risk-notice-short-light", size: firstRunSize)
    }

    // MARK: Inbox detail

    func testLongInboxDetailScrollsBesideTheListLight() async throws {
        let client = PreviewCoreClient()
        client.seedLongInboxItem(id: 100)
        let defaults = UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: defaults), userFirstName: "Sam")
        await model.inbox.load()
        model.inbox.selection = .delivered(100)
        XCTAssertEqual(model.inbox.items.first?.body, PreviewDemo.longInboxBody, "the long item loaded")
        try assertScrollingSnapshot(InboxView(model: model), name: "overflow-inbox-detail-light", size: pageSize)
    }

    // MARK: Approval details

    private func request(detail: String, scopes: Bool = false, tainted: Bool = false) -> ApprovalRequest {
        ApprovalRequest(id: "a-long", tool: "send_email", summary: "Send the Friday summary", risk: "external", detail: detail,
                        alwaysRule: scopes ? "send_email:team@northwind.example" : nil, canAlways: scopes, canSession: scopes,
                        tainted: tainted, outsideSource: tainted ? "an email from billing@northwind.example" : nil,
                        reason: "Heron wants to send the weekly summary you asked for.", sessionID: "s1")
    }

    func testLongApprovalDetailsScrollAndTheButtonsStayVisibleLight() throws {
        let sheet = ApprovalSheetView(request: request(detail: PreviewDemo.longApprovalDetail), showDetails: true) { _ in }
        // The sheet sits in the middle of the smallest main window.
        try assertScrollingSnapshot(sheet.frame(maxWidth: .infinity, maxHeight: .infinity), name: "overflow-approval-details-light", size: windowSize)
    }

    /// The tallest sheets: long details shown, every "How long" row, and the outside-content box.
    func testTallestApprovalSheetsStillFitTheSmallestWindow() {
        let theme = HeronTheme.resolve(theme: .dusk, accent: .system, isDark: false)
        let variants: [(label: String, sheetRequest: ApprovalRequest)] = [
            ("scopes", request(detail: PreviewDemo.longApprovalDetail, scopes: true)),
            ("tainted", request(detail: PreviewDemo.longApprovalDetail, tainted: true)),
        ]
        for variant in variants {
            let sheet = ApprovalSheetView(request: variant.sheetRequest, showDetails: true) { _ in }
                .environment(\.heronTheme, theme)
                .environment(\.heronSnapshotMode, true)
            let hosted = Snapshot.makeHosted(sheet, size: CGSize(width: 520, height: windowSize.height))
            Snapshot.settle(hosted.view)
            XCTAssertLessThanOrEqual(hosted.view.fittingSize.height, windowSize.height,
                                     "the sheet with \(variant.label), buttons included, is no taller than the window")
        }
    }
}

/// `HScroll(maxHeight:)` is as tall as its content up to the cap, and scrolls only past it.
@MainActor
final class HScrollSizingTests: XCTestCase {
    private func height(ofContent contentHeight: CGFloat, cap: CGFloat = 230) -> CGFloat {
        let view = HScroll(maxHeight: cap) { Color.red.frame(height: contentHeight) }.frame(width: 300)
        let hosted = Snapshot.makeHosted(view, size: CGSize(width: 300, height: 600))
        Snapshot.settle(hosted.view)
        return hosted.view.fittingSize.height
    }

    func testShortContentGetsNoEmptySpaceBelowIt() {
        XCTAssertEqual(height(ofContent: 50), 50, accuracy: 1)
    }

    func testContentAtTheCapFillsItExactly() {
        XCTAssertEqual(height(ofContent: 230), 230, accuracy: 1)
    }

    func testTallContentStopsAtTheCapAndScrolls() {
        XCTAssertEqual(height(ofContent: 400), 230, accuracy: 1)
    }

    func testTheRiskBoxCapIs230Points() {
        XCTAssertEqual(RiskNoticeScreen.riskBoxMaxHeight, 230)
    }

    /// The `ImageRenderer` snapshots draw scroll views flat. The flat box must be as tall as its content (and
    /// capped), even in a parent that offers all the height, or snapshots show empty space the app doesn't.
    /// Rendered the way the snapshots are (`ImageRenderer`, one layout pass); the box is the only thing drawn in
    /// pure red, so the number of red rows in the middle column is its height.
    private func flatHeight(ofContent contentHeight: CGFloat, cap: CGFloat = 230) throws -> CGFloat {
        let view = HScroll(maxHeight: cap) { Color(red: 1, green: 0, blue: 0).frame(height: contentHeight) }
            .environment(\.heronFlatScroll, true)
            .frame(width: 300, height: 600, alignment: .topLeading)
            .background(Color.white)
        let image = try Snapshot.render(view, width: 300, height: 600)
        let px = Snapshot.pixels(image)
        let column = image.width / 2
        var red = 0
        for y in 0..<image.height {
            let i = (y * image.width + column) * 4
            if px[i] > 200 && px[i + 1] < 60 && px[i + 2] < 60 { red += 1 }
        }
        return CGFloat(red) * 600 / CGFloat(image.height)
    }

    func testFlatShortContentIsAsTallAsItsContent() throws {
        XCTAssertEqual(try flatHeight(ofContent: 50), 50, accuracy: 2)
    }

    func testFlatTallContentIsClippedAtTheCap() throws {
        XCTAssertEqual(try flatHeight(ofContent: 400), 230, accuracy: 2)
    }
}
