import XCTest
@testable import HeronCore

/// The recorded API responses in `tests/fixtures/api/` (written by Ut from a live core), found from this file's path:
/// Tests/HeronCoreTests/<file> → HeronCoreTests → Tests → HeronMac → macos → repository root.
enum Fixtures {
    static let directory: URL = {
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<5 { url.deleteLastPathComponent() }
        return url.appendingPathComponent("tests/fixtures/api", isDirectory: true)
    }()

    static func data(_ name: String) throws -> Data { try Data(contentsOf: directory.appendingPathComponent(name)) }
    static func text(_ name: String) throws -> String { String(decoding: try data(name), as: UTF8.self) }
    static func names() throws -> [String] {
        try FileManager.default.contentsOfDirectory(atPath: directory.path).filter { !$0.hasPrefix(".") }.sorted()
    }
}

final class ModelDecodingTests: XCTestCase {
    // MARK: Every recorded file

    /// Each fixture file has a decoder here, and decoding it must not throw. A new fixture with no decoder fails
    /// this test, so a recording can't be added and then go unchecked.
    func testEveryFixtureFileDecodes() throws {
        let decoders: [String: (Data) throws -> Void] = [
            "state.json": { _ = try CoreWire.state($0) },
            "sessions.json": { _ = try CoreWire.sessions($0) },
            "session.json": { _ = try CoreWire.session($0) },
            "approvals.json": { _ = try CoreWire.parkedApprovals($0) },
            "audit.json": { _ = try CoreWire.audit($0) },
            "grants.json": { _ = try CoreWire.grants($0) },
            "nevers.json": { _ = try CoreWire.nevers($0) },
            "persona.json": { _ = try CoreWire.persona($0) },
            "providers.json": { _ = try CoreWire.providers($0, settings: nil) },
            "risk-notice.json": { _ = try CoreWire.riskNotice($0) },
            "settings.json": { _ = try CoreWire.settings($0) },
            "today.json": { _ = try CoreWire.today($0) },
            "events-approval.jsonl": { _ = try CoreWire.events(jsonLines: String(decoding: $0, as: UTF8.self)) },
            "events-error.jsonl": { _ = try CoreWire.events(jsonLines: String(decoding: $0, as: UTF8.self)) },
        ]
        let names = try Fixtures.names()
        XCTAssertFalse(names.isEmpty, "no fixtures found at \(Fixtures.directory.path)")
        for name in names {
            guard let decode = decoders[name] else { XCTFail("tests/fixtures/api/\(name) has no decoder in ModelDecodingTests"); continue }
            XCTAssertNoThrow(try decode(try Fixtures.data(name)), name)
        }
        for name in decoders.keys where !names.contains(name) { XCTFail("\(name) is expected in tests/fixtures/api/ but missing") }
    }

    // MARK: Values, so a lenient decoder can't pass by returning defaults

    func testState() throws {
        let s = try CoreWire.state(try Fixtures.data("state.json"))
        XCTAssertEqual(s.name, "Heron")
        XCTAssertEqual(s.version, "0.3.1")
        XCTAssertEqual(s.model, "anthropic/claude-sonnet-5.5")
        XCTAssertEqual(s.mode, .ask)
        XCTAssertEqual(s.sandbox, "seatbelt")
        XCTAssertTrue(s.sandboxOn)
        XCTAssertEqual(s.waiting, 1)
        XCTAssertEqual(s.unread, 0)
        XCTAssertEqual(s.memories, 1)
        XCTAssertFalse(s.onboarded)
        XCTAssertTrue(s.riskNoticeAccepted)
        XCTAssertTrue(s.hasKey)
        XCTAssertFalse(s.paused)
        XCTAssertEqual(s.workspace, "/tmp/heron-fixture/ws")
    }

    func testStateModelLabelIsDecodedFromTheRecording() throws {
        let s = try CoreWire.state(try Fixtures.data("state.json"))
        XCTAssertEqual(s.modelLabel, "anthropic/claude-sonnet-5.5", "the recorded core serves model_label")
        XCTAssertEqual(s.modelDisplayName, s.modelLabel)
    }

    func testStateWithoutAModelLabelFallsBackToTheModel() throws {
        // A core from before model_label: the same recording with the key removed.
        let lines = String(decoding: try Fixtures.data("state.json"), as: UTF8.self).split(separator: "\n", omittingEmptySubsequences: false)
        let json = lines.filter { !$0.contains("\"model_label\"") }.joined(separator: "\n")
        let s = try CoreWire.state(Data(json.utf8))
        XCTAssertNil(s.modelLabel, "an older core doesn't serve it")
        XCTAssertEqual(s.modelDisplayName, "anthropic/claude-sonnet-5.5")
    }

    func testStateModelLabelIsUsedWhenItDiffersFromTheModel() throws {
        let json = String(decoding: try Fixtures.data("state.json"), as: UTF8.self)
            .replacingOccurrences(of: "\"model_label\": \"anthropic/claude-sonnet-5.5\"", with: "\"model_label\": \"Claude Code\"")
        let s = try CoreWire.state(Data(json.utf8))
        XCTAssertEqual(s.modelLabel, "Claude Code")
        XCTAssertEqual(s.modelDisplayName, "Claude Code")
        XCTAssertEqual(s.model, "anthropic/claude-sonnet-5.5", "the model name is still there")
    }

    func testAnEmptyModelLabelFallsBackToTheModel() {
        var s = CoreState(name: "H", version: "1", model: "gpt-4o", modelLabel: "  ", mode: .ask, workspace: "/w", paused: false,
                          waiting: 0, unread: 0, memories: 0, onboarded: true, riskNoticeAccepted: true, sandbox: "seatbelt", hasKey: true)
        XCTAssertEqual(s.modelDisplayName, "gpt-4o")
        s.modelLabel = "Codex"
        XCTAssertEqual(s.modelDisplayName, "Codex")
    }

    func testOAuthAccountsDecodeProvidersAndSkipUnknownOnes() throws {
        let json = #"{"providers":[{"provider":"google","connected":true,"client_configured":true,"account":"sam@gmail.com","read_only":true},{"provider":"microsoft","connected":false,"client_configured":false,"read_only":true},{"provider":"yahoo","connected":false,"client_configured":false}]}"#
        let accounts = try CoreWire.oauthAccounts(Data(json.utf8))
        XCTAssertEqual(accounts, [OAuthAccount(provider: .google, connected: true, clientConfigured: true, account: "sam@gmail.com"),
                                  OAuthAccount(provider: .microsoft, connected: false, clientConfigured: false, account: nil)])
    }

    func testOAuthStartOnlyAcceptsAnHTTPSAddress() throws {
        let url = try CoreWire.oauthStart(Data(#"{"provider":"google","authorization_url":"https://accounts.google.com/o/oauth2/v2/auth?x=1","expires_in_seconds":600}"#.utf8))
        XCTAssertEqual(url.host, "accounts.google.com")
        for bad in ["file:///etc/passwd", "javascript:alert(1)", "http://accounts.example.com/x", "not a url"] {
            XCTAssertThrowsError(try CoreWire.oauthStart(Data(#"{"authorization_url":"\#(bad)"}"#.utf8)), bad)
        }
    }

    func testTheMacCalendarSourceIsARowWhenTheCoreServesIt() throws {
        let json = #"{"calendar_local":{"connected":true,"indexed":12,"learned":0},"documents":{"connected":false,"folders":[],"indexed":0,"learned":0}}"#
        let rows = CoreWire.connections(Data(json.utf8))
        XCTAssertEqual(rows.map(\.id), ["documents", "calendar_local"])
        XCTAssertEqual(rows.last?.detail, "12 events indexed")
        XCTAssertEqual(rows.last?.connected, true)
    }

    func testCountsAreWrittenInTheRightNumber() {
        XCTAssertEqual(countPhrase(1, "item"), "1 item")
        XCTAssertEqual(countPhrase(0, "item"), "0 items")
        XCTAssertEqual(countPhrase(2, "message"), "2 messages")
        XCTAssertEqual(countPhrase(1, "entry", "entries"), "1 entry")
        XCTAssertEqual(countPhrase(2, "entry", "entries"), "2 entries")
        let one = #"{"computer":{"connected":true,"indexed":1},"calendar_local":{"connected":true,"indexed":1},"mail_local":{"connected":true,"indexed":1},"documents":{"connected":true,"folders":["/a"],"indexed":1},"calendar":{"connected":true,"indexed":1,"ics_connected":true,"ics_indexed":1}}"#
        let rows = Dictionary(uniqueKeysWithValues: CoreWire.connections(Data(one.utf8)).map { ($0.id, $0.detail) })
        XCTAssertEqual(rows["computer"], "1 item indexed")
        XCTAssertEqual(rows["calendar"], "1 item indexed")
        XCTAssertEqual(rows["calendar_local"], "1 event indexed")
        XCTAssertEqual(rows["mail_local"], "1 message indexed")
        XCTAssertEqual(rows["documents"], "1 folder, 1 file indexed")
        let many = one.replacingOccurrences(of: "\"indexed\":1", with: "\"indexed\":3").replacingOccurrences(of: "\"ics_indexed\":1", with: "\"ics_indexed\":3")
        let plural = Dictionary(uniqueKeysWithValues: CoreWire.connections(Data(many.utf8)).map { ($0.id, $0.detail) })
        XCTAssertEqual(plural["computer"], "3 items indexed")
        XCTAssertEqual(plural["mail_local"], "3 messages indexed")
        XCTAssertEqual(plural["calendar_local"], "3 events indexed")
        XCTAssertEqual(plural["documents"], "1 folder, 3 files indexed")
    }

    func testTheMacMailSourceIsARowWhenTheCoreServesIt() throws {
        let rows = CoreWire.connections(Data(#"{"mail_local":{"connected":true,"indexed":42,"learned":0}}"#.utf8))
        XCTAssertEqual(rows.map(\.id), ["mail_local"])
        XCTAssertEqual([rows[0].name, rows[0].detail], ["Mac Mail", "42 messages indexed"])
    }

    func testMailAndCalendarRowsReadTheirOwnStateNotTheSignedInAccounts() throws {
        // Google is signed in (the combined flags are true) but no password mailbox and no calendar link exist.
        let json = #"{"email":{"connected":true,"address":null,"indexed":40,"imap_connected":false,"imap_host":null,"imap_indexed":0},"calendar":{"connected":true,"hosts":[],"indexed":9,"ics_connected":false,"ics_indexed":0}}"#
        let rows = CoreWire.connections(Data(json.utf8))
        XCTAssertEqual(rows.map(\.id), ["calendar", "email"])
        XCTAssertEqual(rows.map(\.connected), [false, false])
        XCTAssertEqual(rows[1].name, "iCloud Mail")
    }

    func testAnIcloudMailboxAndAGoogleAccountAreBothThere() throws {
        let icloud = #"{"email":{"connected":true,"address":"sam@icloud.com","indexed":5,"imap_connected":true,"imap_host":"imap.mail.me.com","imap_indexed":5}}"#
        let mail = try XCTUnwrap(CoreWire.connections(Data(icloud.utf8)).first)
        XCTAssertEqual([mail.name, mail.detail, "\(mail.connected)"], ["iCloud Mail", "sam@icloud.com", "true"])
        let other = #"{"email":{"connected":true,"address":"sam@work.example","indexed":5,"imap_connected":true,"imap_host":"mail.work.example","imap_indexed":5}}"#
        XCTAssertEqual(CoreWire.connections(Data(other.utf8)).first?.name, "Mailbox (mail.work.example)")
    }

    func testSandboxNoneIsOff() throws {
        let json = String(decoding: try Fixtures.data("state.json"), as: UTF8.self).replacingOccurrences(of: "\"seatbelt\"", with: "\"none\"")
        XCTAssertFalse(try CoreWire.state(Data(json.utf8)).sandboxOn)
    }

    func testAnUnknownModeFailsLoudly() throws {
        let json = String(decoding: try Fixtures.data("state.json"), as: UTF8.self).replacingOccurrences(of: "\"ask\"", with: "\"reckless\"")
        XCTAssertThrowsError(try CoreWire.state(Data(json.utf8)))
    }

    func testAMissingRequiredFieldFailsLoudly() throws {
        let json = String(decoding: try Fixtures.data("state.json"), as: UTF8.self).replacingOccurrences(of: "\"sandbox\"", with: "\"sandboxx\"")
        XCTAssertThrowsError(try CoreWire.state(Data(json.utf8)))
    }

    func testSessionsAndTheFoldedTranscript() throws {
        let list = try CoreWire.sessions(try Fixtures.data("sessions.json"))
        XCTAssertEqual(list.count, 1)
        XCTAssertEqual(list[0].id, "d7b37882d0ad")
        XCTAssertEqual(list[0].title, "Save a plan as plan.md")
        XCTAssertEqual(list[0].messages, 4)
        XCTAssertEqual(list[0].updatedAt, Date(timeIntervalSince1970: 1_795_043_277))

        // The recording has four raw messages (user, assistant with a call, the tool result, assistant text);
        // the chat shows two: the question, and one reply carrying its step.
        let detail = try CoreWire.session(try Fixtures.data("session.json"))
        XCTAssertEqual(detail.id, "d7b37882d0ad")
        XCTAssertFalse(detail.busy)
        XCTAssertFalse(detail.interrupted)
        XCTAssertEqual(detail.messages.map(\.role), ["user", "assistant"])
        XCTAssertEqual(detail.messages[0].content, "Save a plan as plan.md")
        XCTAssertEqual(detail.messages[1].content, "Done. Here is the result:\n\n```\nwrote 75 bytes to plan.md\n```")
        let step = try XCTUnwrap(detail.messages[1].toolCalls.first)
        XCTAssertEqual(detail.messages[1].toolCalls.count, 1)
        XCTAssertEqual(step.name, "write_file")
        XCTAssertEqual(step.summary, "write file · plan.md")
        XCTAssertEqual(step.ok, true, "the tool message that answers the call marks it done")
    }

    func testAFailedToolResultMarksTheStepFailed() throws {
        let json = """
        {"id":"s","title":"t","busy":false,"interrupted":false,"messages":[
          {"role":"user","content":"x"},
          {"role":"assistant","tool_calls":[{"id":"c1","type":"function","function":{"name":"read_file","arguments":"{\\"path\\":\\"a.txt\\"}"}}]},
          {"role":"tool","tool_call_id":"c1","content":"Error: no such file"}]}
        """
        let detail = try CoreWire.session(Data(json.utf8))
        XCTAssertEqual(detail.messages[1].toolCalls.first?.ok, false)
        XCTAssertEqual(detail.messages[1].content, "")
    }

    func testLiveApprovalsDecodeLikeTheirEvents() throws {
        // The shape `GET /api/approvals/live` returns: the `approval` event plus `session_id` and `run_id`.
        let body = #"""
        [{"type":"approval","id":"9f2c","tool":"write_file","summary":"Write live.txt","risk":"write","detail":"live.txt",
          "always_rule":"write_file:live.txt","can_always":true,"can_session":false,"tainted":true,"session_id":"s1","run_id":"r1"}]
        """#
        let list = try CoreWire.liveApprovals(Data(body.utf8))
        XCTAssertEqual(list.count, 1)
        XCTAssertEqual(list[0].id, "9f2c")
        XCTAssertEqual(list[0].sessionID, "s1")
        XCTAssertEqual(list[0].alwaysRule, "write_file:live.txt")
        XCTAssertTrue(list[0].canAlways)
        XCTAssertFalse(list[0].canSession)
        XCTAssertTrue(list[0].tainted)
        XCTAssertEqual(try CoreWire.liveApprovals(Data("[]".utf8)), [])
        XCTAssertThrowsError(try CoreWire.liveApprovals(Data(#"[{"type":"text","text":"x","session_id":"s1"}]"#.utf8)),
                             "only approval events belong in the list")
    }

    func testParkedApprovals() throws {
        let list = try CoreWire.parkedApprovals(try Fixtures.data("approvals.json"))
        XCTAssertEqual(list.count, 1)
        let a = list[0]
        XCTAssertEqual(a.id, 1)
        XCTAssertEqual(a.tool, "write_file")
        XCTAssertEqual(a.summary, "Write later.txt")
        XCTAssertEqual(a.detail, "later.txt")
        XCTAssertEqual(a.risk, "write")
        XCTAssertEqual(a.sessionID, "d7b37882d0ad")
        XCTAssertEqual(a.origin, "user")
        XCTAssertEqual(a.status, "new")
        XCTAssertFalse(a.tainted)
        XCTAssertFalse(a.isHighRisk)
        XCTAssertEqual(a.timestamp, Date(timeIntervalSince1970: 1_795_043_277))
    }

    func testAudit() throws {
        let log = try CoreWire.audit(try Fixtures.data("audit.json"))
        XCTAssertTrue(log.intact)
        XCTAssertEqual(log.entries.count, 1)
        XCTAssertEqual(log.entries[0].tool, "write_file")
        XCTAssertEqual(log.entries[0].decision, "approved")
        XCTAssertEqual(log.entries[0].provider, "demo")
        XCTAssertEqual(log.entries[0].result, "wrote 75 bytes to plan.md")
        XCTAssertTrue(log.entries[0].isChange)
        XCTAssertFalse(log.entries[0].isRefused)
    }

    func testNeversCarryTheRuleItsToolAndWhereItLives() throws {
        let nevers = try CoreWire.nevers(try Fixtures.data("nevers.json"))
        XCTAssertEqual(nevers, [NeverRule(rule: "web_fetch:*", tool: "web_fetch", inFile: true)])
        // `in_file` false (kept by Heron only) must decode as false, not default to true.
        let kvOnly = try CoreWire.nevers(Data(#"[{"rule":"shell:*sudo*","tool":"shell","in_file":false}]"#.utf8))
        XCTAssertEqual(kvOnly, [NeverRule(rule: "shell:*sudo*", tool: "shell", inFile: false)])
        XCTAssertEqual(try CoreWire.nevers(Data("[]".utf8)), [])
    }

    func testGrantsPersonaAndToday() throws {
        let grants = try CoreWire.grants(try Fixtures.data("grants.json"))
        XCTAssertEqual(grants, [Grant(rule: "shell:git *", kind: "tool")])

        let persona = try CoreWire.persona(try Fixtures.data("persona.json"))
        XCTAssertEqual(persona.name, "Heron")
        XCTAssertTrue(persona.isDefault)
        XCTAssertEqual(persona.maxCharacters, 4000)
        XCTAssertTrue(persona.text.hasPrefix("# Who you are"))

        let today = try CoreWire.today(try Fixtures.data("today.json"))
        XCTAssertEqual(today.waiting, 1)
        XCTAssertEqual(today.events, [])
        XCTAssertEqual(today.dueTasks, [])
        XCTAssertFalse(today.paused)
    }

    func testRiskNoticeIsKeptExactlyAsServed() throws {
        let notice = try CoreWire.riskNotice(try Fixtures.data("risk-notice.json"))
        XCTAssertEqual(notice.version, 1)
        XCTAssertEqual(notice.acceptedVersion, 1)
        XCTAssertTrue(notice.text.hasPrefix("Version: 1\n\n# Accepted risks"))
        XCTAssertTrue(notice.text.contains("## C35-8: Phone turns inherit the desktop permission mode"))
    }

    func testProvidersFollowTheFixtureNulls() throws {
        let list = try CoreWire.providers(try Fixtures.data("providers.json"), settings: try Fixtures.data("settings.json"))
        XCTAssertGreaterThanOrEqual(list.count, 10)
        let claude = try XCTUnwrap(list.first { $0.id == "claude-cli" })
        XCTAssertEqual(claude.displayName, "Claude Code (subscription)")
        XCTAssertTrue(claude.installed)
        XCTAssertEqual(claude.version, "2.1.327")
        XCTAssertFalse(claude.loggedIn, "logged_in is false in the recording")
        XCTAssertFalse(claude.inRange, "in_range is null: there is no tested range yet")
        XCTAssertNil(claude.testedRange)
        XCTAssertEqual(claude.loginCommand, "claude auth login", "from /api/settings")
        XCTAssertEqual(claude.authKind, "Claude Pro/Max subscription")

        let missing = try XCTUnwrap(list.first { $0.id == "cursor-cli" })
        XCTAssertFalse(missing.installed)

        let opencode = try XCTUnwrap(list.first { $0.id == "opencode-cli" })
        XCTAssertTrue(opencode.loggedIn, "logged_in null means it can't be checked, which is treated as signed in")
        XCTAssertEqual(list.filter(\.isActive).count, 0, "the recorded core uses the http provider")
    }

    func testTestedRangeReadsTheCoreRangeString() {
        XCTAssertEqual(CoreWire.testedRange(">=2.0.0 <3.0.0"), "2.0.0 to below 3.0.0")
    }

    func testSettings() throws {
        let s = try CoreWire.settings(try Fixtures.data("settings.json"))
        XCTAssertEqual(s.workspace, "/tmp/heron-fixture/ws")
        XCTAssertEqual(s.mode, .ask)
        XCTAssertEqual(s.ruleNotices, [])
        XCTAssertFalse(s.allowSubscriptionUnattended)
        XCTAssertEqual(s.connections, [])
        XCTAssertFalse(s.awareness)
        XCTAssertFalse(s.phoneConnected)
    }

    // MARK: Event recordings

    func testApprovalRecording() throws {
        let events = try CoreWire.events(jsonLines: try Fixtures.text("events-approval.jsonl"))
        XCTAssertEqual(events.map(\.seq), Array(1...18))
        XCTAssertTrue(events.allSatisfy { $0.sessionID == "session-1" && $0.runID == "run-1" })

        guard case .session(let sid, let rid) = events[0].kind else { return XCTFail("first event is the session") }
        XCTAssertEqual(sid, "session-1")
        XCTAssertEqual(rid, "run-1")

        guard case .approval(let request) = events[2].kind else { return XCTFail("third event is the approval") }
        XCTAssertEqual(request.id, "approval-1")
        XCTAssertEqual(request.tool, "write_file")
        XCTAssertEqual(request.risk, "write")
        XCTAssertEqual(request.summary, "plan.md")
        XCTAssertEqual(request.alwaysRule, "write_file:*")
        XCTAssertTrue(request.canAlways)
        XCTAssertTrue(request.canSession)
        XCTAssertFalse(request.tainted)
        XCTAssertEqual(request.sessionID, "session-1")
        XCTAssertTrue(request.detail.hasPrefix("Create plan.md (75 bytes)\n"))

        XCTAssertEqual(events[3].kind, .approvalResolved(id: "approval-1", decision: "once"))
        XCTAssertEqual(events[4].kind, .toolStart(id: "tool-1", name: "write_file", summary: "plan.md", risk: "write"))
        XCTAssertEqual(events[5].kind, .toolEnd(id: "tool-1", name: "write_file", ok: true, preview: "wrote 75 bytes to plan.md"))
        XCTAssertEqual(events[1].kind, .usage(prompt: 0, completion: 0))
        let streamed = events[6...14].compactMap { (e: CoreEvent) -> String? in if case .text(let t) = e.kind { return t } else { return nil } }.joined()
        XCTAssertEqual(streamed, "Done. Here is the result:\n\n```\nwrote 75 bytes to plan.md\n```")
        XCTAssertEqual(events[16].kind, .done(text: streamed, title: "Save a plan as plan.md"))
        XCTAssertEqual(events[17].kind, .runEnd)
    }

    func testErrorRecording() throws {
        let events = try CoreWire.events(jsonLines: try Fixtures.text("events-error.jsonl"))
        XCTAssertEqual(events.count, 3)
        XCTAssertEqual(events[1].kind, .error(message: "the model is unavailable (scripted failure)", code: nil, provider: nil, version: nil))
        XCTAssertEqual(events[2].kind, .runEnd)
    }

    // MARK: Routes with no recording yet (inline JSON copied from the core's serde structs; unverified against a live core)

    func testErrorEventCarriesCodeProviderAndVersion() throws {
        let json = #"{"seq":4,"session_id":"s","run_id":"r","type":"error","message":"m","code":"cli_version_untested","provider":"claude-cli","version":"2.3.0"}"#
        let e = try XCTUnwrap(CoreWire.envelope(Data(json.utf8)).event)
        XCTAssertEqual(e.kind, .error(message: "m", code: "cli_version_untested", provider: "claude-cli", version: "2.3.0"))
    }

    func testParkedLearnedModelNoticeAndCancelledEvents() throws {
        func kind(_ json: String) throws -> CoreEvent.Kind { try XCTUnwrap(CoreWire.envelope(Data(json.utf8)).event).kind }
        XCTAssertEqual(try kind(#"{"seq":1,"type":"approval_parked","id":"a","parked_id":7}"#), .approvalParked(id: "a", parkedID: 7))
        XCTAssertEqual(try kind(#"{"seq":2,"type":"learned","items":["x","y"]}"#), .learned(["x", "y"]))
        XCTAssertEqual(try kind(#"{"seq":3,"type":"model","name":"fast","why":"short"}"#), .model(name: "fast", why: "short"))
        XCTAssertEqual(try kind(#"{"seq":4,"type":"notice","text":"careful"}"#), .notice("careful"))
        XCTAssertEqual(try kind(#"{"seq":5,"type":"cancelled"}"#), .cancelled)
    }

    func testResyncEnvelopeHasNoSequence() throws {
        let envelope = try CoreWire.envelope(Data(#"{"type":"resync","oldest":6}"#.utf8))
        XCTAssertTrue(envelope.isResync)
        XCTAssertNil(envelope.seq)
        XCTAssertEqual(envelope.event?.kind, .resync(oldest: 6))
    }

    func testUnknownEventTypeKeepsItsSequenceAndYieldsNoEvent() throws {
        let envelope = try CoreWire.envelope(Data(#"{"seq":9,"session_id":"s","run_id":"r","type":"from_the_future"}"#.utf8))
        XCTAssertEqual(envelope.seq, 9)
        XCTAssertNil(envelope.event)
    }

    func testAnEventNeedsAType() {
        XCTAssertThrowsError(try CoreWire.envelope(Data(#"{"seq":1}"#.utf8)))
        XCTAssertThrowsError(try CoreWire.envelope(Data("not json".utf8)))
    }

    func testApprovalEventWithTaintCarriesIt() throws {
        let json = #"{"seq":3,"session_id":"s","run_id":"r","type":"approval","id":"a1","tool":"send_email","summary":"Send","risk":"external","detail":"d","always_rule":null,"can_always":false,"can_session":false,"tainted":true}"#
        guard case .approval(let r) = try XCTUnwrap(CoreWire.envelope(Data(json.utf8)).event).kind else { return XCTFail() }
        XCTAssertTrue(r.tainted)
        XCTAssertTrue(r.isHighRisk)
        XCTAssertNil(r.alwaysRule)
        XCTAssertFalse(r.canAlways)
    }

    func testMemoriesTasksInboxAndSuggestions() throws {
        let memories = try CoreWire.memories(Data(#"[{"id":3,"kind":"fact","content":"Likes tea","tags":"","importance":0.7,"created_at":1795000000,"updated_at":1795000001,"uses":2,"source":"user","confirmed":true}]"#.utf8))
        XCTAssertEqual(memories, [Memory(id: 3, kind: "fact", content: "Likes tea", source: "user", confirmed: true, uses: 2, createdAt: Date(timeIntervalSince1970: 1_795_000_000))])

        let tasks = try CoreWire.tasks(Data(#"[{"id":1,"title":"File taxes","notes":"","status":"open","priority":2,"due":"2026-11-30","created_at":1,"owner":"user"}]"#.utf8))
        XCTAssertEqual(tasks, [TaskItem(id: 1, title: "File taxes", notes: "", status: "open", due: "2026-11-30", owner: "user", progress: nil, completedAt: nil)])

        let inbox = try CoreWire.inbox(Data(#"[{"id":5,"ts":1795000000,"title":"Done","body":"Receipts sorted","read":false}]"#.utf8))
        XCTAssertEqual(inbox.first?.title, "Done")
        XCTAssertEqual(inbox.first?.read, false)

        let suggestions = try CoreWire.suggestions(Data(#"[{"id":2,"ts":1795000000,"title":"Tidy","why":"Downloads is full","prompt":"Tidy Downloads","status":"new","result":null,"session_id":null}]"#.utf8))
        XCTAssertEqual(suggestions.first?.prompt, "Tidy Downloads")
        XCTAssertEqual(suggestions.first?.status, "new")
    }

    func testSchedules() throws {
        let json = #"""
        [{"id":4,"name":"Morning brief","cron":"@daily 07:30","prompt":"Brief me","enabled":true,"next_run":1795100000,"last_run":1795000000,"last_result":"ok","allow":"","quiet":false,"last_tokens":120,"trigger":"Every day at 07:30","outline":null,"uses_model":null},
         {"id":5,"name":"Once","cron":"","prompt":"p","enabled":false,"next_run":null,"last_run":null,"last_result":null,"allow":"write_file:*\nedit_file:*\nshell:*","quiet":true,"last_tokens":0,"trigger":null,"outline":null,"uses_model":null}]
        """#
        let list = try CoreWire.schedules(Data(json.utf8))
        XCTAssertEqual(list[0].trigger, "Every day at 07:30")
        XCTAssertEqual(list[0].permissionsNote, "May only read and look things up")
        XCTAssertEqual(list[0].tokens, 120)
        XCTAssertEqual(list[0].nextRun, Date(timeIntervalSince1970: 1_795_100_000))
        XCTAssertTrue(list[0].enabled)
        XCTAssertEqual(list[1].trigger, "Once")
        XCTAssertEqual(list[1].permissionsNote, "May edit files and run commands")
        XCTAssertNil(list[1].nextRun)
        XCTAssertFalse(list[1].enabled)
    }

    func testTodayEventLinesAreServedAsText() throws {
        let json = #"{"events":["Standup — tomorrow 10:00","[Google Calendar] Dentist — 2026-11-20T09:30:00Z","Team — offsite — next week"],"due_tasks":[{"id":1,"title":"Tax","due":"2026-11-19","overdue":false}],"open_tasks":2,"heron_tasks":1,"waiting":0,"unconfirmed":0,"new_mail":3,"unread":1,"paused":false}"#
        let today = try CoreWire.today(Data(json.utf8))
        XCTAssertEqual(today.events.map(\.title), ["Standup", "Dentist", "Team — offsite"])
        XCTAssertEqual(today.events[0].whenText, "tomorrow 10:00")
        XCTAssertNil(today.events[0].start, "free text is shown as served, not turned into a made-up date")
        XCTAssertEqual(today.events[1].start, Date(timeIntervalSince1970: 1_795_167_000))
        XCTAssertEqual(today.dueTasks, [DueTask(id: 1, title: "Tax", due: "2026-11-19", overdue: false)])
        XCTAssertEqual(today.newMail, 3)
    }

    func testUsageUsesTheModelRows() throws {
        let json = #"{"budget":{"level":"Normal","today":{"used":5,"limit":0,"left":null},"month":{"used":1234,"limit":0,"left":null},"daily_average":1,"projected_month":2,"limit_reached_on":null},"level_label":"Normal","days":[["2026-11-18",5]],"models":[["anthropic/claude-sonnet-5.5",1000,234,7]],"main_model":"m","fast_model":"","routing":false,"background_today":0,"background_limit":0,"session":{},"demo":false}"#
        let usage = try CoreWire.usage(Data(json.utf8))
        XCTAssertEqual(usage.monthTokens, 1234)
        XCTAssertEqual(usage.days, [UsageDay(day: "This month", model: "anthropic/claude-sonnet-5.5", promptTokens: 1000, completionTokens: 234)])
    }

    func testSettingsJoinTheConnectionsAwarenessAndPhoneRoutes() throws {
        let sources = #"{"onboarded":true,"calendar":{"connected":true,"hosts":["x"],"indexed":4,"learned":0},"email":{"connected":true,"address":"me@example.com","indexed":9,"learned":0},"documents":{"connected":false,"folders":[],"indexed":0,"learned":0},"computer":{"connected":false,"indexed":0,"learned":0}}"#
        let s = try CoreWire.settings(try Fixtures.data("settings.json"), sources: Data(sources.utf8),
                                      awareness: Data(#"{"enabled":true}"#.utf8), telegram: Data(#"{"configured":true,"paired":true,"code":null}"#.utf8))
        XCTAssertTrue(s.awareness)
        XCTAssertTrue(s.phoneConnected)
        XCTAssertEqual(s.connections.map(\.id), ["calendar", "email", "documents", "computer"])
        XCTAssertEqual(s.connections[0].detail, "4 items indexed")
        XCTAssertEqual(s.connections[1].detail, "me@example.com")
        XCTAssertEqual(s.connections[2].detail, "Not connected")
        XCTAssertFalse(s.connections[2].connected)
    }

    func testEndpointLine() throws {
        let e = try CoreWire.endpoint(line: #"{"port":51234,"token":"abc","background_owner":false,"version":"0.3.1"}"#)
        XCTAssertEqual(e, CoreEndpoint(port: 51234, token: "abc", backgroundOwner: false, version: "0.3.1"))
        XCTAssertThrowsError(try CoreWire.endpoint(line: "Listening on http://127.0.0.1:1"))
    }
}
