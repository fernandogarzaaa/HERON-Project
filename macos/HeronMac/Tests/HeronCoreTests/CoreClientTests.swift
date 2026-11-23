import XCTest
@testable import HeronCore

/// Answers every request the client makes, so no port is opened. The body is read from `httpBodyStream`:
/// URLSession moves `httpBody` into a stream before the protocol sees the request.
final class StubProtocol: URLProtocol, @unchecked Sendable {
    struct Reply {
        var status = 200
        var headers: [String: String] = ["Content-Type": "application/json"]
        var body = Data()
        init(_ status: Int = 200, json: String = "{}", contentType: String = "application/json") {
            self.status = status
            self.headers = ["Content-Type": contentType]
            self.body = Data(json.utf8)
        }
    }

    struct Seen: Equatable {
        var method: String
        var path: String
        var query: String?
        var token: String?
        var contentType: String?
        var body: String?
    }

    nonisolated(unsafe) static var handler: (@Sendable (Seen) -> Reply)?
    nonisolated(unsafe) static var requests: [Seen] = []
    private static let lock = NSLock()

    static func reset(_ handler: @escaping @Sendable (Seen) -> Reply) {
        lock.withLock {
            self.handler = handler
            requests = []
        }
    }

    static var seen: [Seen] { lock.withLock { requests } }

    static func session() -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [StubProtocol.self]
        return URLSession(configuration: configuration)
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        var body: Data?
        if let stream = request.httpBodyStream {
            stream.open()
            var data = Data()
            var buffer = [UInt8](repeating: 0, count: 4096)
            while stream.hasBytesAvailable {
                let n = stream.read(&buffer, maxLength: buffer.count)
                if n <= 0 { break }
                data.append(buffer, count: n)
            }
            stream.close()
            body = data
        } else {
            body = request.httpBody
        }
        let url = request.url!
        let seen = Seen(method: request.httpMethod ?? "GET", path: url.path, query: url.query(percentEncoded: true),
                        token: request.value(forHTTPHeaderField: "x-heron-token"),
                        contentType: request.value(forHTTPHeaderField: "Content-Type"),
                        body: body.map { String(decoding: $0, as: UTF8.self) })
        let reply: Reply = Self.lock.withLock {
            Self.requests.append(seen)
            return Self.handler?(seen) ?? Reply(404)
        }
        let response = HTTPURLResponse(url: url, statusCode: reply.status, httpVersion: "HTTP/1.1", headerFields: reply.headers)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        if !reply.body.isEmpty { client?.urlProtocol(self, didLoad: reply.body) }
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}

final class CoreClientTests: XCTestCase {
    private let endpoint = CoreEndpoint(port: 4242, token: "secret-token", backgroundOwner: true, version: "0.3.1")

    private func makeClient(_ handler: @escaping @Sendable (StubProtocol.Seen) -> StubProtocol.Reply) -> CoreClient {
        StubProtocol.reset(handler)
        return CoreClient(endpoint: endpoint, session: StubProtocol.session())
    }

    private func body(_ seen: StubProtocol.Seen) throws -> [String: Any] {
        try XCTUnwrap(JSONSerialization.jsonObject(with: Data(try XCTUnwrap(seen.body).utf8)) as? [String: Any])
    }

    func testEveryRequestCarriesTheToken() async throws {
        let state = try Fixtures.text("state.json")
        let client = makeClient { _ in .init(200, json: state) }
        let s = try await client.state()
        XCTAssertEqual(s.sandbox, "seatbelt")
        XCTAssertEqual(StubProtocol.seen.count, 1)
        XCTAssertEqual(StubProtocol.seen[0].method, "GET")
        XCTAssertEqual(StubProtocol.seen[0].path, "/api/state")
        XCTAssertEqual(StubProtocol.seen[0].token, "secret-token")
    }

    func testJSONBodiesAreSentWithAContentType() async throws {
        let client = makeClient { _ in .init(200, json: #"{"ok":true}"#) }
        try await client.approve(id: "approval-1", decision: .always)
        let seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual(seen.method, "POST")
        XCTAssertEqual(seen.path, "/api/approve")
        XCTAssertEqual(seen.contentType, "application/json")
        let json = try body(seen)
        XCTAssertEqual(json["id"] as? String, "approval-1")
        XCTAssertEqual(json["decision"] as? String, "always")
    }

    func testDeleteGrantSendsTheRuleInTheBody() async throws {
        let client = makeClient { _ in .init(200, json: #"{"ok":true,"rule":"shell:git *"}"#) }
        try await client.revokeGrant(rule: "shell:git *")
        let seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual(seen.method, "DELETE")
        XCTAssertEqual(seen.path, "/api/grants")
        XCTAssertEqual(try body(seen)["rule"] as? String, "shell:git *")
    }

    func testNeversAreListedWithGetAndRemovedWithDeleteAndTheRule() async throws {
        let list = try Fixtures.text("nevers.json")
        let client = makeClient { _ in .init(200, json: list) }
        let nevers = try await client.nevers()
        XCTAssertEqual(nevers.map(\.rule), ["web_fetch:*"])
        var seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual(seen.method, "GET")
        XCTAssertEqual(seen.path, "/api/nevers")
        XCTAssertEqual(seen.token, "secret-token")

        let remover = makeClient { _ in .init(200, json: #"{"ok":true}"#) }
        try await remover.removeNever(rule: "web_fetch:*example.com*")
        seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual(seen.method, "DELETE")
        XCTAssertEqual(seen.path, "/api/nevers")
        XCTAssertEqual(seen.token, "secret-token")
        XCTAssertEqual(seen.contentType, "application/json")
        XCTAssertEqual(try body(seen)["rule"] as? String, "web_fetch:*example.com*")
    }

    func testMacMailMessagesGoToTheMailLocalRoute() async throws {
        let client = makeClient { _ in .init(200, json: #"{"indexed":1}"#) }
        try await client.connectSource(.localMail(messages: [LocalMailMessage(id: "<a@x>", from: "Dana", subject: "Hi", date: 1_795_100_000, account: "iCloud", body: "Text")]))
        let seen = try XCTUnwrap(StubProtocol.seen.last)
        XCTAssertEqual([seen.method, seen.path], ["POST", "/api/sources/mail_local"])
        let sent = try XCTUnwrap(try body(seen)["messages"] as? [[String: Any]])
        XCTAssertEqual(sent.count, 1)
        XCTAssertEqual([sent[0]["id"] as? String, sent[0]["account"] as? String, sent[0]["body"] as? String], ["<a@x>", "iCloud", "Text"])
        XCTAssertEqual(sent[0]["date"] as? Int, 1_795_100_000)
        XCTAssertEqual(SourceConnection.localMail(messages: []).sourceID, "mail_local")
        XCTAssertFalse("\(SourceConnection.localMail(messages: [LocalMailMessage(id: "i", date: 1, body: "secret text")]))".contains("secret text"), "the text is not printed")
    }

    func testICloudMailIsTheEmailSourceWithIcloudsServerFilledIn() async throws {
        let client = makeClient { _ in .init(200, json: #"{"indexed":3}"#) }
        try await client.connectSource(.iCloudMail(address: "sam@icloud.com", password: "abcd-efgh-ijkl-mnop"))
        let seen = StubProtocol.seen
        XCTAssertEqual(seen.map(\.path), ["/api/sources/email"])
        let sent = try body(seen[0])
        XCTAssertEqual(sent["host"] as? String, "imap.mail.me.com")
        XCTAssertEqual(sent["username"] as? String, "sam@icloud.com")
        XCTAssertEqual(sent["port"] as? Int, 993)
        XCTAssertEqual(sent["tls"] as? Bool, true)
        XCTAssertFalse("\(SourceConnection.iCloudMail(address: "sam@icloud.com", password: "abcd-efgh-ijkl-mnop"))".contains("abcd"), "the password is never printed")
    }

    func testConnectingEachSourceSendsItsFieldsToItsRoute() async throws {
        let client = makeClient { _ in .init(200, json: #"{"indexed":3}"#) }
        try await client.connectSource(.documents(path: "/Users/sam/Notes"))
        try await client.connectSource(.calendar(url: "https://example.com/private.ics"))
        try await client.connectSource(.email(host: "imap.gmail.com", username: "sam@example.com", password: "app-pass", port: nil, tls: nil))
        try await client.connectSource(.computer)
        let seen = StubProtocol.seen
        XCTAssertEqual(seen.map(\.method), ["POST", "POST", "POST", "POST"])
        XCTAssertEqual(seen.map(\.path), ["/api/sources/documents", "/api/sources/calendar", "/api/sources/email", "/api/sources/computer"])
        XCTAssertEqual(seen.map(\.token), Array(repeating: "secret-token", count: 4))
        XCTAssertEqual(try body(seen[0])["path"] as? String, "/Users/sam/Notes")
        XCTAssertEqual(try body(seen[1])["url"] as? String, "https://example.com/private.ics")
        let email = try body(seen[2])
        XCTAssertEqual(email["host"] as? String, "imap.gmail.com")
        XCTAssertEqual(email["username"] as? String, "sam@example.com")
        XCTAssertEqual(email["password"] as? String, "app-pass")
        XCTAssertNil(email["port"], "the core's defaults (993, TLS) apply when the app says nothing")
        XCTAssertNil(email["tls"])
        XCTAssertEqual(seen[3].contentType, "application/json", "the scan sends an empty object, which axum's Json needs")
        XCTAssertEqual(try body(seen[3]).count, 0)
    }

    func testEmailPortAndTLSAreSentWhenGiven() async throws {
        let client = makeClient { _ in .init(200, json: #"{"indexed":0}"#) }
        try await client.connectSource(.email(host: "mail.example.com", username: "a@example.com", password: "p", port: 143, tls: false))
        let json = try body(try XCTUnwrap(StubProtocol.seen.first))
        XCTAssertEqual(json["port"] as? Int, 143)
        XCTAssertEqual(json["tls"] as? Bool, false)
    }

    func testMacCalendarEventsAreSentAsJSONToTheirSource() async throws {
        let client = makeClient { _ in .init(200, json: #"{"indexed":2}"#) }
        try await client.connectSource(.localCalendar(events: [
            LocalCalendarEvent(uid: "e1", title: "Dentist", start: 1_795_100_000, end: 1_795_101_800, allDay: false, location: "Main St"),
            LocalCalendarEvent(uid: "e2", title: "Holiday", start: 1_795_200_000, end: nil, allDay: true),
        ]))
        let seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual(seen.method, "POST")
        XCTAssertEqual(seen.path, "/api/sources/calendar_local")
        let events = try XCTUnwrap(try body(seen)["events"] as? [[String: Any]])
        XCTAssertEqual(events.count, 2)
        XCTAssertEqual(events[0]["uid"] as? String, "e1")
        XCTAssertEqual(events[0]["start"] as? Int, 1_795_100_000)
        XCTAssertEqual(events[0]["end"] as? Int, 1_795_101_800)
        XCTAssertEqual(events[0]["location"] as? String, "Main St")
        XCTAssertEqual(events[1]["all_day"] as? Bool, true)
        XCTAssertNil(events[1]["end"], "an event with no end sends none")
    }

    func testOAuthRoutes() async throws {
        let status = #"{"providers":[{"provider":"google","connected":false,"client_configured":true,"read_only":true}]}"#
        var client = makeClient { _ in .init(200, json: status) }
        let accounts = try await client.oauthAccounts()
        XCTAssertEqual(accounts.map(\.provider), [.google])
        var seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual([seen.method, seen.path, seen.token ?? ""], ["GET", "/api/oauth", "secret-token"])

        client = makeClient { _ in .init(200, json: #"{"provider":"google","authorization_url":"https://accounts.google.com/o/oauth2/v2/auth?state=s","expires_in_seconds":600}"#) }
        let url = try await client.startOAuth(provider: .google)
        XCTAssertEqual(url.host, "accounts.google.com")
        seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual([seen.method, seen.path], ["POST", "/api/oauth/google/start"])
        XCTAssertEqual(seen.contentType, "application/json", "the core's Json extractor needs a body")

        client = makeClient { _ in .init(200, json: #"{"emails":3,"events":4}"#) }
        try await client.syncOAuth(provider: .microsoft)
        seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual([seen.method, seen.path], ["POST", "/api/oauth/microsoft/sync"])

        client = makeClient { _ in .init(200, json: #"{"removed_items":1,"removed_memories":0}"#) }
        try await client.disconnectOAuth(provider: .google)
        seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual([seen.method, seen.path], ["DELETE", "/api/oauth/google"])
    }

    func testAnOAuthStartThatTheCoreRefusesCarriesItsSentence() async throws {
        let client = makeClient { _ in .init(400, json: #"{"error":"This build has no public desktop OAuth client ID."}"#) }
        do { _ = try await client.startOAuth(provider: .google); XCTFail("expected the failure") } catch {
            XCTAssertEqual(error as? CoreError, .failed("This build has no public desktop OAuth client ID."))
        }
    }

    func testDisconnectingASourceDeletesItsRoute() async throws {
        let client = makeClient { _ in .init(200, json: #"{"removed_items":4,"removed_memories":1}"#) }
        try await client.disconnectSource(id: "email")
        let seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual(seen.method, "DELETE")
        XCTAssertEqual(seen.path, "/api/sources/email")
        XCTAssertEqual(seen.token, "secret-token")
    }

    func testASourceFailureCarriesTheCoresSentence() async throws {
        let client = makeClient { _ in .init(400, json: #"{"error":"Heron could not find that folder. Check the path and try again."}"#) }
        do { try await client.connectSource(.documents(path: "/nope")); XCTFail("expected the failure") } catch {
            XCTAssertEqual(error as? CoreError, .failed("Heron could not find that folder. Check the path and try again."))
        }
    }

    func testAnEmailSourceDescriptionNeverPrintsThePassword() {
        let source = SourceConnection.email(host: "imap.gmail.com", username: "sam@example.com", password: "hunter2-app-pass", port: nil, tls: nil)
        XCTAssertFalse("\(source)".contains("hunter2-app-pass"))
    }

    func testRemovingARuleThatIsNotANeverIsNotFound() async throws {
        let client = makeClient { _ in .init(404, json: #"{"error":"That rule is not a never."}"#) }
        do { try await client.removeNever(rule: "nope:*"); XCTFail("expected 404") } catch {
            XCTAssertEqual(error as? CoreError, .notFound)
        }
    }

    func testStatusCodesBecomeTheErrorsScreensReactTo() async throws {
        let busy = makeClient { _ in .init(409, json: #"{"error":"session_busy","message":"Heron is still replying."}"#) }
        do { _ = try await busy.sendMessage(sessionID: "s1", message: "hi", title: nil); XCTFail("expected sessionBusy") } catch {
            XCTAssertEqual(error as? CoreError, .sessionBusy)
        }
        let blocked = makeClient { _ in .init(428, json: #"{"error":"risk_notice_not_accepted"}"#) }
        do { _ = try await blocked.sendMessage(sessionID: nil, message: "hi", title: nil); XCTFail("expected 428") } catch {
            XCTAssertEqual(error as? CoreError, .riskNoticeNotAccepted)
        }
        let missing = makeClient { _ in .init(404, json: #"{"error":"That request is no longer waiting for an answer."}"#) }
        do { try await missing.approve(id: "gone", decision: .once); XCTFail("expected 404") } catch {
            XCTAssertEqual(error as? CoreError, .notFound)
        }
        let other = makeClient { _ in .init(400, json: #"{"error":"Type a message first."}"#) }
        do { try await other.setPaused(true); XCTFail("expected a failure") } catch {
            XCTAssertEqual(error as? CoreError, .failed("Type a message first."))
        }
    }

    func testRiskNoticeAcceptanceHandles204And409() async throws {
        let ok = makeClient { _ in .init(204, json: "") }
        try await ok.acceptRiskNotice(version: 1)
        XCTAssertEqual(try body(try XCTUnwrap(StubProtocol.seen.first))["version"] as? Int, 1)

        let outdated = makeClient { _ in .init(409, json: #"{"error":"Version 1 is not the current risk notice (2). Reload it and read it again."}"#) }
        do { try await outdated.acceptRiskNotice(version: 1); XCTFail("expected 409") } catch {
            XCTAssertEqual(error as? CoreError, .riskNoticeOutdated)
        }
    }

    func testSendMessageReturnsAtTheSessionEventAndSendsNoTitle() async throws {
        let sse = """
        id: 1
        data: {"seq":1,"session_id":"sess-9","run_id":"run-9","type":"session","id":"sess-9"}

        id: 2
        data: {"seq":2,"session_id":"sess-9","run_id":"run-9","type":"text","text":"hi"}


        """
        let client = makeClient { _ in .init(200, json: sse, contentType: "text/event-stream") }
        let start = try await client.sendMessage(sessionID: nil, message: "Hello", title: "Ignored title")
        XCTAssertEqual(start, ChatStart(sessionID: "sess-9", runID: "run-9"))
        let json = try body(try XCTUnwrap(StubProtocol.seen.first))
        XCTAssertEqual(json["message"] as? String, "Hello")
        XCTAssertNil(json["session_id"], "a new conversation sends no session id")
        XCTAssertNil(json["title"], "/api/chat has no title field")
    }

    func testSendMessageIntoAnExistingConversation() async throws {
        let sse = "id: 1\ndata: {\"seq\":1,\"session_id\":\"abc\",\"run_id\":\"r\",\"type\":\"session\",\"id\":\"abc\"}\n\n"
        let client = makeClient { _ in .init(200, json: sse, contentType: "text/event-stream") }
        _ = try await client.sendMessage(sessionID: "abc", message: "more", title: nil)
        XCTAssertEqual(try body(try XCTUnwrap(StubProtocol.seen.first))["session_id"] as? String, "abc")
    }

    /// The sidebar's menu: rename, pin and archive go to `POST /api/sessions/{id}` with only what changed.
    func testSessionUpdatesSendOnlyWhatChanged() async throws {
        let client = makeClient { _ in .init(200, json: "{}") }
        try await client.updateSession(id: "a b", title: "Taxes", pinned: nil, archived: nil)
        try await client.updateSession(id: "s1", title: nil, pinned: true, archived: false)
        let seen = StubProtocol.seen
        XCTAssertEqual(seen.map(\.method), ["POST", "POST"])
        XCTAssertEqual(seen[0].path, "/api/sessions/a b", "the id travels as one path segment")
        XCTAssertEqual(try body(seen[0]) as NSDictionary, ["title": "Taxes"])
        XCTAssertEqual(try body(seen[1]) as NSDictionary, ["pinned": true, "archived": false])
    }

    func testArchivedSessionsAskForTheArchivedListAndDecodeTheMarks() async throws {
        let list = #"[{"id":"s1","title":"Old","created_at":1,"updated_at":1795003200,"messages":3,"pinned":true,"archived":true}]"#
        let client = makeClient { _ in .init(200, json: list) }
        let archived = try await client.archivedSessions()
        XCTAssertEqual(StubProtocol.seen.first?.path, "/api/sessions")
        XCTAssertEqual(StubProtocol.seen.first?.query, "archived=true")
        XCTAssertEqual(archived.map(\.pinned), [true])
        XCTAssertEqual(archived.map(\.archived), [true])
        // A core older than the menu sends neither mark.
        let older = try CoreWire.sessions(Data(#"[{"id":"s1","title":"Old","updated_at":1,"messages":3}]"#.utf8))
        XCTAssertEqual(older.map(\.pinned), [false])
        XCTAssertEqual(older.map(\.archived), [false])
    }

    func testQueriesAndPaths() async throws {
        let client = makeClient { _ in .init(200, json: "[]") }
        _ = try await client.memories(query: "tea & milk")
        _ = try await client.memories(query: "  ")
        let seen = StubProtocol.seen
        XCTAssertEqual(seen[0].path, "/api/memories")
        XCTAssertEqual(seen[0].query, "q=tea%20%26%20milk", "the ampersand is escaped")
        XCTAssertNil(seen[1].query, "a blank query is no query")
    }

    func testTaskDoneAndScheduleRoutes() async throws {
        let client = makeClient { _ in .init(200, json: #"{"updated":true,"started":true,"id":5}"#) }
        try await client.setTaskDone(id: 3, done: true)
        try await client.runSchedule(id: 8)
        try await client.answerParked(id: 2, action: .dismiss)
        try await client.answerSuggestion(id: 4, answer: .accept)
        let id = try await client.addTask(title: "File taxes", due: "2026-11-30")
        XCTAssertEqual(id, 5)
        let seen = StubProtocol.seen
        XCTAssertEqual(seen.map(\.path), ["/api/tasks/3", "/api/schedules/8/run", "/api/approvals/2", "/api/suggestions/4", "/api/tasks"])
        XCTAssertEqual(try body(seen[0])["status"] as? String, "done")
        XCTAssertEqual(try body(seen[2])["action"] as? String, "dismiss")
        XCTAssertEqual(try body(seen[3])["action"] as? String, "accept")
        XCTAssertEqual(try body(seen[4])["due"] as? String, "2026-11-30")
    }

    func testSettingsRoutes() async throws {
        let client = makeClient { _ in .init(200, json: "{}") }
        try await client.selectProvider(id: "codex-cli")
        try await client.setUnattendedSubscription(true)
        try await client.setAwareness(true)
        try await client.setOnboarded()
        try await client.acceptProviderVersion(id: "claude-cli", version: "2.3.0")
        let seen = StubProtocol.seen
        XCTAssertEqual(seen.map(\.path), ["/api/settings", "/api/settings", "/api/awareness", "/api/onboarded", "/api/providers/claude-cli/accept-version"])
        XCTAssertEqual(try body(seen[0])["provider"] as? String, "codex-cli")
        XCTAssertEqual(try body(seen[1])["allow_unattended_subscription"] as? Bool, true)
        XCTAssertEqual(try body(seen[2])["enabled"] as? Bool, true)
        XCTAssertEqual(try body(seen[4])["version"] as? String, "2.3.0")
    }

    func testProvidersJoinTheSettingsRoute() async throws {
        let providers = try Fixtures.text("providers.json")
        let settings = try Fixtures.text("settings.json")
        let client = makeClient { seen in seen.path == "/api/providers" ? .init(200, json: providers) : .init(200, json: settings) }
        let list = try await client.providers()
        XCTAssertEqual(list.first { $0.id == "claude-cli" }?.loginCommand, "claude auth login")
    }

    func testSettingsStillLoadWhenTheConnectionRoutesFail() async throws {
        let settings = try Fixtures.text("settings.json")
        let client = makeClient { seen in seen.path == "/api/settings" ? .init(200, json: settings) : .init(500, json: #"{"error":"boom"}"#) }
        let s = try await client.settings()
        XCTAssertEqual(s.connections, [])
        XCTAssertFalse(s.awareness)
    }

    func testPausingASingleScheduleIsAKnownGap() async throws {
        let client = makeClient { _ in .init(200) }
        do { try await client.setScheduleEnabled(id: 1, enabled: false); XCTFail("expected an error") } catch {
            guard case CoreError.failed(let message) = error else { return XCTFail("\(error)") }
            XCTAssertTrue(message.contains("can't pause or resume"), message)
        }
        XCTAssertEqual(StubProtocol.seen.count, 0, "no request that the core would answer with 400")
    }

    func testEventsConnectionAsksForSinceAndYieldsLines() async throws {
        let client = makeClient { _ in .init(200, json: "id: 3\ndata: {\"seq\":3,\"type\":\"run_end\"}\n\n", contentType: "text/event-stream") }
        let bytes = try await client.openEventConnection(since: 2)
        var text = ""
        for try await chunk in bytes { text += String(decoding: chunk, as: UTF8.self) }
        XCTAssertEqual(text, "id: 3\ndata: {\"seq\":3,\"type\":\"run_end\"}\n\n")
        XCTAssertEqual(StubProtocol.seen.first?.path, "/api/events")
        XCTAssertEqual(StubProtocol.seen.first?.query, "since=2")
        XCTAssertEqual(StubProtocol.seen.first?.token, "secret-token")
    }
}

final class ManagedCoreClientTests: XCTestCase {
    func testACallMadeBeforeTheEndpointWaitsForIt() async throws {
        let core = try FakeCore(.waitForStdin)
        let process = CoreProcess(launch: .init(executable: core.script))
        defer { process.stop(grace: 1) }
        let state = try Fixtures.text("state.json")
        StubProtocol.reset { _ in .init(200, json: state) }
        let managed = ManagedCoreClient(process: process, makeClient: { CoreClient(endpoint: $0, session: StubProtocol.session()) })

        let pending = Task { try await managed.state() } // before the core was started
        try await Task.sleep(for: .milliseconds(100))
        process.start()
        let s = try await pending.value
        XCTAssertEqual(s.waiting, 1)
        XCTAssertEqual(managed.endpoint.port, 9001)
        XCTAssertEqual(StubProtocol.seen.first?.token, "token1", "the request used the endpoint the core printed")
    }

    func testCallsFailOnceTheCoreHasStoppedForGood() async throws {
        let core = try FakeCore(.exitAtOnce)
        let process = CoreProcess(launch: .init(executable: core.script), now: StepClock(step: 0).now)
        defer { process.stop(grace: 0.2) }
        let stream = process.statuses()
        let managed = ManagedCoreClient(process: process, makeClient: { CoreClient(endpoint: $0, session: StubProtocol.session()) })
        StubProtocol.reset { _ in .init(200, json: "{}") }
        process.start()
        _ = try await collect(stream, until: { isStopped($0.last) })
        try await Task.sleep(for: .milliseconds(200))
        do { _ = try await managed.sessions(); XCTFail("expected a failure") } catch {
            guard case CoreError.coreStopped(let details) = error else { return XCTFail("\(error)") }
            XCTAssertTrue(details.contains("exited with status 0"), details)
        }
    }

    /// Sending a message is the user asking Heron to work, so a stopped core is started again for it, and the
    /// message goes to the new core instead of failing with the old core's details.
    func testSendingWhileTheCoreIsStoppedStartsItAgain() async throws {
        let core = try FakeCore(.exitFirst(2))
        let process = CoreProcess(launch: .init(executable: core.script), now: StepClock(step: 0).now)
        defer { process.stop(grace: 0.5) }
        let stream = process.statuses()
        let managed = ManagedCoreClient(process: process, makeClient: { CoreClient(endpoint: $0, session: StubProtocol.session()) })
        StubProtocol.reset { _ in .init(200, json: "data: {\"seq\":1,\"session_id\":\"s1\",\"run_id\":\"r1\",\"type\":\"session\",\"id\":\"s1\"}\n\n", contentType: "text/event-stream") }
        process.start()
        _ = try await collect(stream, until: { isStopped($0.last) })
        try await Task.sleep(for: .milliseconds(200))
        let start = try await managed.sendMessage(sessionID: "s1", message: "hello", title: nil)
        XCTAssertEqual(start.sessionID, "s1")
        XCTAssertEqual(StubProtocol.seen.last?.token, "token3", "the message went to the restarted core")
        if case .running = process.status {} else { XCTFail("the core runs again: \(process.status)") }
    }

    /// A core that can't start at all: the one retry for the message fails, and says the core isn't running.
    func testSendingFailsPlainlyWhenTheCoreCannotStartAgain() async throws {
        let missing = FileManager.default.temporaryDirectory.appendingPathComponent("heron-no-such-core-\(UUID().uuidString)")
        let process = CoreProcess(launch: .init(executable: missing), now: StepClock(step: 0).now)
        defer { process.stop(grace: 0.2) }
        let stream = process.statuses()
        let managed = ManagedCoreClient(process: process, makeClient: { CoreClient(endpoint: $0, session: StubProtocol.session()) })
        process.start()
        _ = try await collect(stream, until: { isStopped($0.last) })
        try await Task.sleep(for: .milliseconds(200))
        let after = process.statuses()
        do { _ = try await managed.sendMessage(sessionID: nil, message: "hello", title: nil); XCTFail("expected a failure") } catch {
            guard case CoreError.coreStopped(let details) = error else { return XCTFail("\(error)") }
            XCTAssertTrue(details.contains("couldn't start"), details)
        }
        let seen = try await collect(after, until: { isStopped($0.last) && $0.count > 1 })
        XCTAssertTrue(seen.contains(.starting), "the message started the core once more: \(seen)")
    }
}
