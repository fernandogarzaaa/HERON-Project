import XCTest
@testable import HeronCore
@testable import HeronUI

/// F2, "Why is it asking?": the chip's sentence, the new fields decoded from the core's wire shapes, the chat store
/// following the `tainted` event, the chip's switch, and F2's row in Settings › Features.
@MainActor
final class TaintChipTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_795_003_200)

    private func makeClient() async throws -> PreviewCoreClient {
        let client = PreviewCoreClient(endpoint: CoreEndpoint(port: 1, token: "t", backgroundOwner: true, version: "test"), now: now)
        try await client.acceptRiskNotice(version: 3)
        return client
    }

    private func defaults() -> UserDefaults { UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")! }

    /// `tests/fixtures/api/<name>`, found from this file's path:
    /// Features → HeronUITests → Tests → HeronMac → macos → repository root.
    private func fixture(_ name: String) throws -> Data {
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<6 { url.deleteLastPathComponent() }
        return try Data(contentsOf: url.appendingPathComponent("tests/fixtures/api/\(name)"))
    }

    private let preview = TaintSource(kind: "chat_context", name: "selection", detail: "Preview")

    // MARK: The sentence (the core's `approval_text::taint_sentence`)

    func testTheSentenceNamesWhatHeronReadForEachKindOfSource() {
        let cases: [(TaintSource, String)] = [
            (TaintSource(kind: "tool", name: "web_fetch", detail: "127.0.0.1"), "127.0.0.1"),
            (TaintSource(kind: "tool", name: "web_fetch", detail: ""), "the output of web_fetch"),
            (preview, "text from Preview"),
            (TaintSource(kind: "mcp", name: "mcp", detail: "conn-1"), "a request from another app over MCP"),
            (TaintSource(kind: "telegram_forward", name: "telegram"), "a forwarded Telegram message"),
            (TaintSource(kind: "trigger", name: "Invoices", detail: "email"), "the event that started Invoices"),
            (TaintSource(kind: "suggestion", name: "suggestion"), "a task Heron prepared from what it observed"),
        ]
        for (source, what) in cases {
            XCTAssertEqual(TaintChipCopy.sentence(source), "Heron has read \(what) in this conversation, so changes and sends ask first.", "\(source)")
        }
    }

    func testTheDesignsExampleSentenceForAPreviewSelection() {
        XCTAssertEqual(TaintChipCopy.sentence(preview), "Heron has read text from Preview in this conversation, so changes and sends ask first.")
    }

    func testNoSourceOrAKindThisVersionDoesNotKnowGivesTheOlderSentence() {
        let older = "Heron has read content from outside (a web page, an email or a connected app) in this conversation. Check that this is something you asked for."
        XCTAssertEqual(TaintChipCopy.noSource, older)
        XCTAssertEqual(TaintChipCopy.sentence(nil), older)
        XCTAssertEqual(TaintChipCopy.sentence(TaintSource(kind: "carrier_pigeon", name: "x", detail: "y")), older)
        XCTAssertNil(TaintChipCopy.what(TaintSource(kind: "carrier_pigeon")))
    }

    // MARK: Decoding

    func testTheRecordedSessionSaysItIsNotTaintedAndTheRecordedApprovalCarriesWhy() throws {
        let raw = try XCTUnwrap(try JSONSerialization.jsonObject(with: try fixture("session.json")) as? [String: Any])
        let taint = try XCTUnwrap(raw["taint"] as? [String: Any], "the re-recorded session has a `taint` object")
        XCTAssertEqual(taint["tainted"] as? Bool, false)
        let detail = try CoreWire.session(try fixture("session.json"))
        XCTAssertFalse(detail.tainted)
        XCTAssertNil(detail.taintSource)

        let text = String(decoding: try fixture("events-approval.jsonl"), as: UTF8.self)
        let approval = try XCTUnwrap(try CoreWire.events(jsonLines: text).compactMap { e -> ApprovalRequest? in
            if case .approval(let r) = e.kind { return r } else { return nil }
        }.first)
        XCTAssertEqual(approval.why, "Heron asks before this in ask mode.")
        XCTAssertFalse(approval.tainted)
    }

    func testASessionTaintWithASourceDecodesAllThreeFields() throws {
        let json = #"{"id":"s","title":"t","busy":false,"interrupted":false,"messages":[],"taint":{"tainted":true,"source":{"kind":"tool","name":"web_fetch","detail":"127.0.0.1","ts":5}}}"#
        let detail = try CoreWire.session(Data(json.utf8))
        XCTAssertTrue(detail.tainted)
        XCTAssertEqual(detail.taintSource, TaintSource(kind: "tool", name: "web_fetch", detail: "127.0.0.1"))
    }

    func testASessionTaintedBeforeSourcesWereRecordedIsTaintedWithNoSource() throws {
        let json = #"{"id":"s","title":"t","busy":false,"interrupted":false,"messages":[],"taint":{"tainted":true,"source":null}}"#
        let detail = try CoreWire.session(Data(json.utf8))
        XCTAssertTrue(detail.tainted)
        XCTAssertNil(detail.taintSource)
    }

    func testTheTaintedEventDecodesItsSource() throws {
        let json = #"{"seq":3,"session_id":"s1","run_id":"r1","type":"tainted","source":{"kind":"chat_context","name":"selection","detail":"Preview","ts":1795000000}}"#
        let event = try XCTUnwrap(CoreWire.envelope(Data(json.utf8)).event)
        XCTAssertEqual(event.kind, .tainted(preview))
        XCTAssertEqual(event.sessionID, "s1")
        XCTAssertEqual(event.seq, 3)
    }

    func testATaintedEventWithNoSourceFailsLoudly() {
        XCTAssertThrowsError(try CoreWire.envelope(Data(#"{"seq":3,"type":"tainted"}"#.utf8)))
        XCTAssertThrowsError(try CoreWire.envelope(Data(#"{"seq":3,"type":"tainted","source":{"name":"x"}}"#.utf8)))
    }

    func testAnApprovalWithNoWhyDecodesWithNone() throws {
        let json = #"{"seq":3,"session_id":"s1","type":"approval","id":"a","tool":"write_file","summary":"x","risk":"write","tainted":false}"#
        guard case .approval(let request)? = try CoreWire.envelope(Data(json.utf8)).event?.kind else { return XCTFail("an approval") }
        XCTAssertNil(request.why, "ask_reasons off: the core leaves `why` out")
    }

    func testATaintedApprovalCarriesTheSecondSentence() throws {
        let why = "This acts on a website or another app. Heron cannot undo it. Heron has read text from Preview in this conversation, so changes and sends ask first."
        let json = #"{"seq":3,"session_id":"s1","type":"approval","id":"a","tool":"web_post","summary":"x","risk":"external","tainted":true,"why":"\#(why)"}"#
        guard case .approval(let request)? = try CoreWire.envelope(Data(json.utf8)).event?.kind else { return XCTFail("an approval") }
        XCTAssertEqual(request.why, why)
        XCTAssertTrue(request.why?.hasSuffix(TaintChipCopy.sentence(preview)) ?? false, "the sheet and the chip say the same sentence")
    }

    // MARK: The approval sheet's line

    func testTheSheetShowsTheWhyTrimmedAndNothingWhenItIsEmptyOrMissing() {
        func request(_ why: String?) -> ApprovalRequest {
            ApprovalRequest(id: "a", tool: "write_file", summary: "x", risk: "write", detail: "", alwaysRule: nil, canAlways: false, canSession: false,
                            tainted: false, outsideSource: nil, reason: nil, sessionID: "s", why: why)
        }
        XCTAssertEqual(ApprovalCopy.whyLine(request("  Heron asks before this in ask mode.\n")), "Heron asks before this in ask mode.")
        XCTAssertNil(ApprovalCopy.whyLine(request(nil)))
        XCTAssertNil(ApprovalCopy.whyLine(request("")))
        XCTAssertNil(ApprovalCopy.whyLine(request("  \n ")))
    }

    // MARK: The chat store

    func testTheTaintedEventMarksTheConversationAndTheFirstSourceStays() async throws {
        let chat = ChatStore(client: try await makeClient(), now: { self.now })
        await chat.send("hello")
        let sid = try XCTUnwrap(chat.currentSessionID)
        XCTAssertFalse(chat.isOutsideContent)
        XCTAssertNil(chat.taintSource)

        chat.handle(CoreEvent(seq: 2, sessionID: sid, runID: "r", kind: .tainted(preview)))
        XCTAssertTrue(chat.isOutsideContent)
        XCTAssertEqual(chat.taintSource, preview)

        chat.handle(CoreEvent(seq: 3, sessionID: sid, runID: "r", kind: .tainted(TaintSource(kind: "mcp", name: "mcp", detail: "c"))))
        XCTAssertEqual(chat.taintSource, preview, "the core keeps the first source, so the chip does too")
    }

    func testATaintedEventForAnotherConversationChangesNothing() async throws {
        let chat = ChatStore(client: try await makeClient(), now: { self.now })
        await chat.send("hello")
        chat.handle(CoreEvent(seq: 2, sessionID: "someone-else", runID: "r", kind: .tainted(preview)))
        XCTAssertFalse(chat.isOutsideContent)
        XCTAssertNil(chat.taintSource)
    }

    func testOpeningAConversationTakesItsTaintAndItsSourceAndANewOneClearsThem() async throws {
        let client = try await makeClient()
        client.seedTaintedConversation(id: "s-with", source: preview)
        client.seedTaintedConversation(id: "s-legacy", source: nil)
        let chat = ChatStore(client: client, now: { self.now })

        await chat.open(id: "s-with")
        XCTAssertTrue(chat.isOutsideContent)
        XCTAssertEqual(chat.taintSource, preview)

        await chat.open(id: "s-legacy")
        XCTAssertTrue(chat.isOutsideContent, "tainted before sources were recorded: still tainted")
        XCTAssertNil(chat.taintSource, "and the last conversation's source doesn't carry over")

        await chat.open(id: "s-with")
        chat.startNewConversation()
        XCTAssertFalse(chat.isOutsideContent)
        XCTAssertNil(chat.taintSource)
    }

    // MARK: The chip's switch

    func testTheChipShowsOnlyForATaintedConversationWhileExplainApprovalsIsOn() async throws {
        let client = try await makeClient()
        client.seedTaintedConversation()
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: defaults()),
                             features: PreviewFeaturesClient(items: [CoreFeature(key: "ask_reasons", title: "Explain approvals", on: true)]))
        await model.chat.open(id: "s-tainted")
        XCTAssertTrue(model.chat.isOutsideContent)
        XCTAssertFalse(model.showsTaintChip, "the core hasn't said whether the key is on")

        await model.features.load()
        XCTAssertTrue(model.showsTaintChip)

        await model.features.set(AskReasonsFeature.row, on: false)
        XCTAssertFalse(model.showsTaintChip, "off: the chip hides, as the core leaves `why` out")

        await model.features.set(AskReasonsFeature.row, on: true)
        XCTAssertTrue(model.showsTaintChip)
        model.chat.startNewConversation()
        XCTAssertFalse(model.showsTaintChip, "a conversation that has read nothing outside has no chip")
    }

    func testARoutedTaintedEventReachesTheChat() async throws {
        let client = try await makeClient()
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: defaults()),
                             features: PreviewFeaturesClient(items: [CoreFeature(key: "ask_reasons", title: "Explain approvals", on: true)]))
        await model.chat.send("hello")
        let sid = try XCTUnwrap(model.chat.currentSessionID)
        model.route(CoreEvent(seq: 2, sessionID: sid, runID: "r", kind: .tainted(preview)))
        XCTAssertEqual(model.chat.taintSource, preview)
        XCTAssertTrue(model.chat.isOutsideContent)
    }

    // MARK: F2's row

    func testF2HasOneCoreRowInSettingsFeaturesUnderTheCoresTitle() async throws {
        XCTAssertEqual(FeatureRows.all.filter { $0.key == "ask_reasons" }.count, 1)
        let row = try XCTUnwrap(FeatureRows.all.first { $0.key == "ask_reasons" })
        XCTAssertEqual(row, AskReasonsFeature.row)
        XCTAssertEqual(row.title, "Explain approvals")
        XCTAssertEqual(row.kind, .core(key: "ask_reasons"))
        XCTAssertNil(row.permission, "F2 needs no macOS permission")

        let store = FeaturesSettingsStore(client: PreviewFeaturesClient(), defaults: defaults())
        await store.load()
        XCTAssertEqual(store.items.filter { $0.row.key == "ask_reasons" }.count, 1, "a key with a row isn't listed twice")
        XCTAssertEqual(store.items.first { $0.row.key == "ask_reasons" }?.on, true, "on by default")
    }
}
