import XCTest
import HeronCore
@testable import HeronUI

/// Each store against the `PreviewCoreClient` stub.
@MainActor
final class ScreenStoreTests: XCTestCase {
    /// 2026-11-18 12:00 UTC (a Wednesday); the stub's relative dates and the stores' due tags use the same value.
    private let now = Date(timeIntervalSince1970: 1_795_003_200)

    private func makeClient(accepted: Bool = true, backgroundOwner: Bool = true) async throws -> PreviewCoreClient {
        let endpoint = CoreEndpoint(port: 1, token: "t", backgroundOwner: backgroundOwner, version: "test")
        let client = PreviewCoreClient(endpoint: endpoint, now: now)
        if accepted { try await client.acceptRiskNotice(version: 3) }
        return client
    }

    /// Feeds events to `handle` until the run ends, calling `onApproval` when an approval arrives.
    private func pump(_ stream: AsyncStream<CoreEvent>, handle: (CoreEvent) -> Void, onApproval: (() async -> Void)? = nil) async {
        for await event in stream {
            handle(event)
            if case .approval = event.kind { await onApproval?() }
            if case .runEnd = event.kind { return }
        }
    }

    // MARK: Chat

    func testChatSendsAMessageAndBuildsTheReplyWithItsSteps() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client, now: { self.now })
        let stream = client.events(since: nil)
        chat.composer = "What is on today?"
        await chat.send()
        XCTAssertEqual(chat.composer, "", "the composer clears once the run starts")
        XCTAssertTrue(chat.isRunning)
        XCTAssertEqual(chat.messages.map(\.role), [.user, .assistant])
        await pump(stream, handle: chat.handle)
        XCTAssertFalse(chat.isRunning)
        let reply = try XCTUnwrap(chat.messages.last)
        XCTAssertTrue(reply.text.contains("three events"))
        XCTAssertTrue(reply.finished)
        XCTAssertEqual(reply.steps.map(\.name), ["upcoming_events"])
        XCTAssertEqual(reply.steps.first?.status, .done)
        XCTAssertEqual(chat.sessions.first?.title, "What is on today?")
    }

    /// Feeds `stream` to `chat` from inside `sendMessage`, before it returns, until the run ends.
    private func deliverBeforeSendReturns(_ client: PreviewCoreClient, _ stream: AsyncStream<CoreEvent>, to chat: ChatStore) {
        client.runBeforeSendReturns { _ in
            for await event in stream {
                await MainActor.run { chat.handle(event) }
                if case .runEnd = event.kind { return }
            }
        }
    }

    func testEventsThatArriveBeforeSendReturnsBuildTheNewReply() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client, now: { self.now })
        deliverBeforeSendReturns(client, client.events(since: nil), to: chat)
        await chat.send("What is on today?")
        XCTAssertEqual(chat.messages.map(\.role), [.user, .assistant])
        let reply = try XCTUnwrap(chat.messages.last)
        XCTAssertTrue(reply.text.contains("three events"), "the early text reached the new reply: \(reply.text)")
        XCTAssertTrue(reply.finished)
        XCTAssertEqual(reply.steps.map(\.name), ["upcoming_events"])
        XCTAssertFalse(chat.isRunning, "a run_end that came early still ends the run, so the composer is free")
        XCTAssertTrue(chat.canSend || chat.composer.isEmpty)
    }

    func testEarlyEventsOfASecondTurnNeverReachThePreviousReply() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client, now: { self.now })
        let first = client.events(since: nil)
        await chat.send("What is on today?")
        await pump(first, handle: chat.handle)
        let previous = try XCTUnwrap(chat.messages.last)
        deliverBeforeSendReturns(client, client.events(since: nil), to: chat)
        await chat.send("What is on today?")
        XCTAssertEqual(chat.messages.count, 4)
        XCTAssertEqual(chat.messages[1], previous, "the earlier reply is unchanged")
        XCTAssertTrue(chat.messages[3].text.contains("three events"))
        XCTAssertTrue(chat.messages[3].finished)
        XCTAssertFalse(chat.isRunning)
    }

    func testResyncRestoresTheLiveApprovalOfTheOpenConversation() async throws {
        let client = try await makeClient()
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!),
                             userFirstName: "Sam", now: { self.now })
        model.selection = .chat
        let stream = client.events(since: nil)
        await model.chat.send("Draft a reply to Maria")
        // The app falls behind the bus: the approval event is never routed.
        var missed: ApprovalRequest?
        for await event in stream { if case .approval(let r) = event.kind { missed = r; break } }
        let request = try XCTUnwrap(missed)
        XCTAssertNil(model.chat.liveApproval)
        let stale = ApprovalRequest(id: "gone", tool: "write_file", summary: "s", risk: "write", detail: "", alwaysRule: nil,
                                    canAlways: false, canSession: false, tainted: false, outsideSource: nil, reason: nil, sessionID: "other")
        model.approvals.add(stale)
        await model.resynchronize()
        XCTAssertEqual(model.chat.liveApproval?.id, request.id, "the sheet comes back, so the user can answer it")
        XCTAssertTrue(model.chat.isRunning)
        XCTAssertTrue(model.approvals.live.isEmpty, "the feed holds only what the core waits on now")
        await model.chat.answer(.once)
        XCTAssertNil(model.chat.liveApproval)
        let live = try await client.liveApprovals()
        XCTAssertTrue(live.isEmpty)
    }

    func testResyncPutsAnotherConversationsLiveApprovalInTheFeed() async throws {
        let client = try await makeClient()
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!),
                             userFirstName: "Sam", now: { self.now })
        let stream = client.events(since: nil)
        model.selection = .chat
        await model.chat.send("Draft a reply to Maria")
        for await event in stream { if case .approval = event.kind { break } }
        model.newConversation()
        await model.resynchronize()
        XCTAssertNil(model.chat.liveApproval)
        XCTAssertEqual(model.approvals.live.count, 1)
        let waiting = try XCTUnwrap(model.approvals.live.first)
        try await client.approve(id: waiting.id, decision: .deny)
    }

    func testChatLiveApprovalShowsThenContinuesAfterAnswer() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client, now: { self.now })
        let stream = client.events(since: nil)
        await chat.send("Draft a reply to Maria")
        var sawSheet = false
        await pump(stream, handle: chat.handle) {
            sawSheet = chat.liveApproval != nil
            XCTAssertEqual(chat.messages.last?.steps.last?.status, .waiting)
            XCTAssertTrue(chat.messages.last?.steps.last?.sentence.hasPrefix("Waiting for you: ") ?? false)
            XCTAssertEqual(chat.composerPlaceholder, "Heron is waiting for your answer above.")
            XCTAssertFalse(chat.canSend)
            await chat.answer(.once)
        }
        XCTAssertTrue(sawSheet)
        XCTAssertNil(chat.liveApproval)
        XCTAssertFalse(chat.answeredElsewhere, "answered here, so no 'Answered from your phone' note")
        XCTAssertTrue(chat.messages.last?.text.contains("saved the draft") ?? false)
    }

    func testChatDenyEndsWithoutRunningTheTool() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client)
        let stream = client.events(since: nil)
        await chat.send("Draft a reply")
        await pump(stream, handle: chat.handle) { await chat.answer(.deny) }
        XCTAssertTrue(chat.messages.last?.text.contains("didn't save") ?? false)
        XCTAssertFalse(chat.messages.last?.steps.contains { $0.name == "write_file" } ?? true)
    }

    func testApprovalAnsweredElsewhereClosesTheSheetAndNotes() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client)
        let stream = client.events(since: nil)
        await chat.send("Draft a reply")
        var elsewhereID: String?
        await pump(stream, handle: chat.handle) {
            // Another window answers: the client resolves it, and this store never answered.
            elsewhereID = chat.liveApproval?.id
            try? await client.approve(id: elsewhereID!, decision: .once)
        }
        XCTAssertNotNil(elsewhereID)
        XCTAssertNil(chat.liveApproval)
        XCTAssertTrue(chat.answeredElsewhere)
        chat.clearAnsweredElsewhere()
        XCTAssertFalse(chat.answeredElsewhere)
    }

    func testParkedApprovalMovesStepToInbox() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client)
        await chat.send("hello")
        let sid = try XCTUnwrap(chat.currentSessionID)
        chat.handle(CoreEvent(seq: 2, sessionID: sid, runID: "r", kind: .toolStart(id: "t", name: "send_email", summary: "Sending", risk: "external")))
        let request = ApprovalRequest(id: "a1", tool: "send_email", summary: "Send", risk: "external", detail: "", alwaysRule: nil,
                                      canAlways: false, canSession: false, tainted: false, outsideSource: nil, reason: nil, sessionID: sid)
        chat.handle(CoreEvent(seq: 3, sessionID: sid, runID: "r", kind: .approval(request)))
        XCTAssertNotNil(chat.liveApproval)
        chat.handle(CoreEvent(seq: 4, sessionID: sid, runID: "r", kind: .approvalParked(id: "a1", parkedID: 9)))
        XCTAssertNil(chat.liveApproval)
        XCTAssertEqual(chat.messages.last?.steps.last?.status, .movedToInbox)
        XCTAssertEqual(chat.messages.last?.steps.last?.sentence, "Moved to your Inbox")
    }

    func testTaintedApprovalMarksTheConversationAsOutsideContent() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client)
        await chat.send("hello")
        let sid = try XCTUnwrap(chat.currentSessionID)
        var request = ApprovalRequest(id: "a2", tool: "write_file", summary: "Create a file", risk: "write", detail: "x", alwaysRule: nil,
                                      canAlways: false, canSession: false, tainted: true, outsideSource: "an email from billing@northwind.example",
                                      reason: nil, sessionID: sid)
        request.tainted = true
        chat.handle(CoreEvent(sessionID: sid, runID: "r", kind: .approval(request)))
        XCTAssertTrue(chat.isOutsideContent)
        XCTAssertEqual(chat.outsideSource, "an email from billing@northwind.example")
    }

    func testBusyConversationKeepsTheMessageAndShowsTheBusyCard() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client)
        await chat.send("Draft something")   // parks at its approval, so the session stays busy
        chat.composer = "second message"
        await chat.send()
        XCTAssertEqual(chat.composer, "second message", "a busy conversation keeps the message in the box")
        XCTAssertEqual(chat.composerError?.title, "Heron is still working on this conversation")
        XCTAssertEqual(chat.composerError?.actions, [.stopReply])
        await chat.stop()
    }

    func testRiskNoticeNotAcceptedAsksForTheNotice() async throws {
        let client = try await makeClient(accepted: false)
        let chat = ChatStore(client: client)
        await chat.send("hello")
        XCTAssertTrue(chat.needsRiskNotice)
        XCTAssertTrue(chat.messages.isEmpty, "nothing was sent")
        chat.clearRiskNoticeRequest()
        XCTAssertFalse(chat.needsRiskNotice)
    }

    func testErrorEventsBecomeCardsFromTheirCode() async throws {
        let untested = ChatErrorCard.from(message: "x", code: "cli_version_untested", provider: "Claude Code", version: "3.0.1")
        XCTAssertEqual(untested.title, "Untested version 3.0.1 of Claude Code")
        XCTAssertEqual(untested.actions, [.continueUntested(provider: "Claude Code", version: "3.0.1"), .modelSettings])
        let folder = ChatErrorCard.from(message: "x", code: "workspace_refused", provider: nil, version: nil)
        XCTAssertEqual(folder.actions, [.chooseFolder])
        XCTAssertTrue(folder.text.contains("holds secrets"))
        let loggedOut = ChatErrorCard.from(message: "Claude Code is logged out", code: nil, provider: "Claude Code", version: nil)
        XCTAssertEqual(loggedOut.actions, [.copyCommand("claude auth login"), .tryAgain])
        let other = ChatErrorCard.from(message: "The model timed out", code: nil, provider: nil, version: nil)
        XCTAssertEqual(other.title, "The model timed out", "never a generic 'Something went wrong'")
    }

    func testFailingRunShowsTheErrorUnderTheReply() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client)
        let stream = client.events(since: nil)
        await chat.send("this will fail")
        await pump(stream, handle: chat.handle)
        let card = try XCTUnwrap(chat.messages.last?.error)
        XCTAssertTrue(card.title.hasPrefix("Untested version 3.0.1"))
        XCTAssertFalse(chat.isRunning)
    }

    func testOpeningASessionLoadsItsMessages() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client)
        await chat.loadSessions()
        let id = try XCTUnwrap(chat.sessions.first?.id)
        await chat.open(id: id)
        XCTAssertEqual(chat.messages.count, 2)
        XCTAssertEqual(chat.messages.first?.role, .user)
        XCTAssertEqual(chat.loadState, .loaded)
        chat.startNewConversation()
        XCTAssertTrue(chat.messages.isEmpty)
        XCTAssertNil(chat.currentSessionID)
    }

    func testRunStatusLineBeforeTheFirstText() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client)
        await chat.send("hi")
        XCTAssertEqual(chat.runStatusLine, "Thinking. Esc stops Heron.")
        let sid = try XCTUnwrap(chat.currentSessionID)
        chat.handle(CoreEvent(sessionID: sid, runID: "r", kind: .toolStart(id: "t9", name: "upcoming_events", summary: "Reading your calendar…", risk: "safe")))
        XCTAssertEqual(chat.runStatusLine, "Reading your calendar…")
        chat.handle(CoreEvent(sessionID: sid, runID: "r", kind: .text("Here")))
        XCTAssertNil(chat.runStatusLine)
    }

    // MARK: Approvals

    func testApprovalsStoreLoadsParkedAndAnswersThem() async throws {
        let client = try await makeClient()
        let store = ApprovalsStore(client: client)
        await store.load()
        XCTAssertEqual(store.parked.count, 1)
        XCTAssertEqual(store.waitingCount, 1)
        let live = ApprovalRequest(id: "live-1", tool: "write_file", summary: "Create a file", risk: "write", detail: "d", alwaysRule: nil,
                                   canAlways: false, canSession: false, tainted: false, outsideSource: nil, reason: nil, sessionID: "other")
        store.add(live)
        store.add(live)
        XCTAssertEqual(store.live.count, 1, "an approval is never listed twice")
        XCTAssertEqual(store.waitingCount, 2)
        store.handle(CoreEvent(kind: .approvalResolved(id: "live-1", decision: "once")))
        XCTAssertTrue(store.live.isEmpty)
        await store.answerParked(store.parked[0], .dismiss)
        XCTAssertTrue(store.parked.isEmpty)
        XCTAssertEqual(store.waitingCount, 0)
    }

    // MARK: Inbox

    func testInboxCombinesThreeSourcesAndFilters() async throws {
        let client = try await makeClient()
        let inbox = InboxStore(client: client, approvals: ApprovalsStore(client: client))
        await inbox.load()
        XCTAssertEqual(inbox.approvals.parked.count, 1)
        XCTAssertEqual(inbox.suggestions.count, 1)
        XCTAssertEqual(inbox.items.count, 2)
        XCTAssertEqual(inbox.unreadCount, 2, "one unread item and one new suggestion")
        XCTAssertFalse(inbox.isEmpty)
        inbox.filter = .waiting
        XCTAssertTrue(inbox.showsWaiting)
        XCTAssertFalse(inbox.showsDelivered)
        inbox.filter = .delivered
        XCTAssertFalse(inbox.showsWaiting)
        XCTAssertTrue(inbox.showsDelivered)
        await inbox.markAllRead()
        XCTAssertEqual(inbox.items.filter { !$0.read }.count, 0)
        await inbox.answer(inbox.suggestions[0], .dismiss)
        XCTAssertTrue(inbox.suggestions.isEmpty)
    }

    // MARK: Today

    func testTodaySentencesComeFromCounts() async throws {
        XCTAssertEqual(TodayStore.summarySentence(events: 3, dueTasks: 2, waiting: 1), "Three events, two tasks due, and one thing waiting for you.")
        XCTAssertEqual(TodayStore.summarySentence(events: 1, dueTasks: 0, waiting: 0), "One event.")
        XCTAssertEqual(TodayStore.summarySentence(events: 0, dueTasks: 2, waiting: 3), "Two tasks due and three things waiting for you.")
        XCTAssertEqual(TodayStore.summarySentence(events: 12, dueTasks: 0, waiting: 0), "12 events.")
        XCTAssertEqual(TodayStore.summarySentence(events: 0, dueTasks: 0, waiting: 0), "Nothing on the calendar, nothing due, and nothing waiting for you.")
        var utc = Calendar(identifier: .gregorian); utc.timeZone = TimeZone(identifier: "UTC")!
        let store = TodayStore(client: try await makeClient(), userFirstName: "Sam")
        XCTAssertEqual(store.greeting(now: now.addingTimeInterval(-4 * 3600), calendar: utc), "Good morning, Sam.")
        XCTAssertEqual(store.greeting(now: now.addingTimeInterval(-1), calendar: utc), "Good morning, Sam.", "11:59 is still morning")
        XCTAssertEqual(store.greeting(now: now, calendar: utc), "Good afternoon, Sam.")
        XCTAssertEqual(store.greeting(now: now.addingTimeInterval(6 * 3600 - 1), calendar: utc), "Good afternoon, Sam.", "17:59 is still afternoon")
        XCTAssertEqual(store.greeting(now: now.addingTimeInterval(6 * 3600), calendar: utc), "Good evening, Sam.")
        store.userFirstName = ""
        XCTAssertEqual(store.greeting(now: now.addingTimeInterval(-4 * 3600), calendar: utc), "Good morning.")
    }

    func testTodayLoadsSummaryAndPrepPromptIsTheApprovedText() async throws {
        let client = try await makeClient()
        let store = TodayStore(client: client, userFirstName: "Sam")
        await store.load()
        XCTAssertEqual(store.summary?.events.count, 3)
        XCTAssertEqual(store.summary?.dueTasks.count, 2, "one overdue and one due today")
        XCTAssertEqual(store.summary?.dueTasks.filter(\.overdue).count, 1)
        XCTAssertEqual(store.summary?.waiting, 1)
        XCTAssertTrue(store.subtitle(now: now).hasSuffix("Three events, two tasks due, and one thing waiting for you."))
        XCTAssertTrue(TodayStore.prepPrompt.hasPrefix("Prep my day."))
        XCTAssertTrue(TodayStore.prepPrompt.hasSuffix("Never send, delete or schedule anything."))
        XCTAssertEqual(TodayStore.prepTitle(now: now), "Prep · Wed 18 Nov")
    }

    // MARK: Memory

    func testMemoryFiltersConfirmsForgetsAndUndoes() async throws {
        let client = try await makeClient()
        let store = MemoryStore(client: client)
        await store.load()
        XCTAssertEqual(store.memories.count, 4)
        XCTAssertEqual(store.toCheck.count, 1)
        XCTAssertEqual(store.confirmed.count, 3)
        store.filter = .people
        XCTAssertEqual(store.visible.map(\.kind), ["person"])
        store.filter = .toCheck
        XCTAssertEqual(store.visible.count, 1)
        store.filter = .all
        await store.confirm(try XCTUnwrap(store.toCheck.first))
        XCTAssertEqual(store.toCheck.count, 0)
        let victim = try XCTUnwrap(store.confirmed.first { $0.kind == "preference" })
        await store.forget(victim)
        XCTAssertEqual(store.lastForgotten?.id, victim.id)
        XCTAssertFalse(store.memories.contains { $0.id == victim.id })
        await store.undoForget()
        XCTAssertNil(store.lastForgotten)
        XCTAssertTrue(store.memories.contains { $0.content == victim.content })
    }

    func testMemorySearchAndEmptyStates() async throws {
        let client = try await makeClient()
        let store = MemoryStore(client: client)
        store.query = "dentist"
        await store.load()
        XCTAssertTrue(store.noResults)
        XCTAssertFalse(store.isFirstUse)
        await store.clearSearch()
        XCTAssertEqual(store.query, "")
        XCTAssertFalse(store.noResults)
        store.query = "northwind"
        await store.load()
        XCTAssertEqual(store.memories.count, 2)
    }

    func testMemoryEditReplacesTheText() async throws {
        let client = try await makeClient()
        let store = MemoryStore(client: client)
        await store.load()
        let m = try XCTUnwrap(store.memories.first { $0.kind == "person" })
        await store.edit(m, content: "  Maria Alvarez leads Northwind procurement.  ")
        XCTAssertTrue(store.memories.contains { $0.content == "Maria Alvarez leads Northwind procurement." })
        XCTAssertFalse(store.memories.contains { $0.id == m.id })
        let count = store.memories.count
        await store.edit(try XCTUnwrap(store.memories.first), content: "x")
        XCTAssertEqual(store.memories.count, count, "text under three characters is ignored")
    }

    // MARK: Tasks

    func testTasksSectionsAddAndToggle() async throws {
        let client = try await makeClient()
        let store = TasksStore(client: client, now: { self.now })
        await store.load()
        XCTAssertEqual(store.yours.count, 3)
        XCTAssertEqual(store.heronDoing.map(\.progress), ["14 of 22 receipts"])
        XCTAssertEqual(store.doneThisWeek.count, 1)
        store.newTitle = "   "
        await store.add()
        XCTAssertEqual(store.yours.count, 3, "an empty title adds nothing")
        store.newTitle = "Call the dentist"
        await store.add()
        XCTAssertEqual(store.yours.count, 4)
        XCTAssertEqual(store.newTitle, "")
        await store.toggle(try XCTUnwrap(store.yours.first))
        XCTAssertEqual(store.yours.count, 3)
        XCTAssertEqual(store.doneThisWeek.count, 2)
        // Eight days later neither completed task is "this week" any more.
        let later = TasksStore(client: client, now: { self.now.addingTimeInterval(8 * 86_400) })
        await later.load()
        XCTAssertEqual(later.doneThisWeek.count, 0)
        XCTAssertEqual(later.tasks.filter { $0.status == "done" }.count, 2)
    }

    func testDueTags() {
        var cal = Calendar(identifier: .gregorian); cal.timeZone = TimeZone(identifier: "UTC")!
        XCTAssertEqual(DueTag.make(due: "2026-11-17", now: now, calendar: cal), .overdue)
        XCTAssertEqual(DueTag.make(due: "2026-11-18", now: now, calendar: cal), .today)
        XCTAssertEqual(DueTag.make(due: "2026-11-20", now: now, calendar: cal), .weekday("Fri"))
        XCTAssertEqual(DueTag.make(due: nil, now: now, calendar: cal), .none)
        XCTAssertEqual(DueTag.make(due: "soon", now: now, calendar: cal), .none)
        XCTAssertEqual(DueTag.overdue.title, "Overdue")
        XCTAssertEqual(DueTag.none.title, "No date")
    }

    // MARK: Schedules

    func testSchedulesSplitActiveAndHeldAndRun() async throws {
        let client = try await makeClient()
        let store = SchedulesStore(client: client)
        await store.load()
        XCTAssertEqual(store.active.count, 2)
        XCTAssertEqual(store.held.count, 1)
        XCTAssertTrue(store.held[0].heldReason?.hasPrefix("Held: Claude Code is a subscription login") ?? false)
        await store.runNow(store.active[0])
        XCTAssertEqual(store.schedules.first { $0.id == 1 }?.lastResult, "Ran just now.")
        await store.setEnabled(store.held[0], true)
        XCTAssertEqual(store.held.count, 0)
        XCTAssertEqual(store.active.count, 3)
    }

    // MARK: Activity

    func testActivityFiltersAndSentences() async throws {
        let client = try await makeClient()
        let store = ActivityStore(client: client)
        await store.load()
        XCTAssertEqual(store.entries.count, 3)
        store.filter = .refused
        XCTAssertEqual(store.entries.map(\.tool), ["shell"])
        store.filter = .changes
        XCTAssertEqual(store.entries.map(\.tool), ["write_file"], "refused actions changed nothing, reads aren't changes")
        XCTAssertEqual(ActivityStore.sentence(store.log.entries[0]), "Wrote ~/Work/drafts/northwind-reply.md")
        XCTAssertEqual(ActivityStore.decisionLabel(store.log.entries[0]), "Approved")
        XCTAssertEqual(ActivityStore.decisionLabel(store.log.entries[1]), "Allowed by mode")
        XCTAssertEqual(ActivityStore.decisionLabel(store.log.entries[2]), "Refused")
        XCTAssertTrue(store.log.intact)
    }

    // MARK: Settings

    // MARK: Settings: connecting sources

    private func connectionsStore() async throws -> (SettingsStore, PreviewCoreClient) {
        let client = try await makeClient()
        let store = SettingsStore(client: client)
        await store.load()
        return (store, client)
    }

    func testConnectingASourceReloadsTheRows() async throws {
        let (store, client) = try await connectionsStore()
        XCTAssertEqual(store.settings?.connections.first { $0.id == "documents" }?.connected, false)
        let ok = await store.connect(.documents(path: "/Users/sam/Notes"))
        XCTAssertTrue(ok)
        XCTAssertEqual(store.settings?.connections.first { $0.id == "documents" }?.connected, true)
        XCTAssertNil(store.sourceErrors["documents"])
        XCTAssertFalse(store.isBusy(source: "documents"), "the spinner is gone")
        XCTAssertEqual(client.sourceRequests, [.documents(path: "/Users/sam/Notes")])
    }

    func testConnectingICloudMailKeepsASignedInGoogleAccountAndSendsOnlyTheEmailRequest() async throws {
        let (store, client) = try await connectionsStore()
        let ok = await store.connect(.iCloudMail(address: "sam@icloud.com", password: "abcd-efgh-ijkl-mnop"))
        XCTAssertTrue(ok)
        XCTAssertEqual(client.sourceRequests, [.iCloudMail(address: "sam@icloud.com", password: "abcd-efgh-ijkl-mnop")])
        XCTAssertEqual(store.settings?.connections.first { $0.id == "email" }?.connected, true)
        XCTAssertTrue(client.oauthSyncs.isEmpty, "no account was touched")
    }

    func testICloudCopyNamesWhereToMakeTheAppPassword() {
        XCTAssertEqual(SourceCopy.iCloudURL, "https://account.apple.com")
        XCTAssertTrue(SourceCopy.iCloudWhy.contains("App-Specific Passwords"))
    }

    func testAFailedConnectShowsTheCoresSentenceOnThatRowAndChangesNothing() async throws {
        let (store, client) = try await connectionsStore()
        client.failNextSourceChange(with: .failed("Couldn't sign in to imap.gmail.com. Check the app password."))
        let ok = await store.connect(.email(host: "imap.gmail.com", username: "sam@example.com", password: "wrong", port: nil, tls: nil))
        XCTAssertFalse(ok)
        XCTAssertEqual(store.sourceErrors["email"], "Couldn't sign in to imap.gmail.com. Check the app password.")
        XCTAssertNil(store.sourceErrors["calendar"], "other rows are untouched")
        XCTAssertFalse(store.isBusy(source: "email"))
        store.clearSourceError("email")
        XCTAssertNil(store.sourceErrors["email"])
    }

    func testTheNextConnectClearsTheLastError() async throws {
        let (store, client) = try await connectionsStore()
        client.failNextSourceChange(with: .failed("Paste your calendar's private ICS link."))
        _ = await store.connect(.calendar(url: "x"))
        XCTAssertNotNil(store.sourceErrors["calendar"])
        let ok = await store.connect(.calendar(url: "https://example.com/p.ics"))
        XCTAssertTrue(ok)
        XCTAssertNil(store.sourceErrors["calendar"])
    }

    func testThisMacScanConnectsItsRow() async throws {
        let (store, _) = try await connectionsStore()
        let ok = await store.connect(.computer)
        XCTAssertTrue(ok)
        XCTAssertEqual(store.settings?.connections.first { $0.id == "computer" }?.connected, true)
    }

    func testDisconnectAsksFirstAndChangesNothingUntilConfirmed() async throws {
        let (store, client) = try await connectionsStore()
        store.askToDisconnect("email")
        XCTAssertEqual(store.pendingDisconnect?.id, "email")
        var rows = try await client.settings().connections
        XCTAssertEqual(rows.first { $0.id == "email" }?.connected, true, "asking disconnects nothing")
        store.cancelDisconnect()
        XCTAssertNil(store.pendingDisconnect)
        store.askToDisconnect("email")
        await store.confirmDisconnect()
        XCTAssertNil(store.pendingDisconnect)
        rows = try await client.settings().connections
        XCTAssertEqual(rows.first { $0.id == "email" }?.connected, false)
        XCTAssertEqual(store.settings?.connections.first { $0.id == "email" }?.connected, false, "the page reloaded")
    }

    func testAFailedDisconnectKeepsTheRowConnectedAndSaysWhy() async throws {
        let (store, client) = try await connectionsStore()
        client.failNextSourceChange(with: .failed("An OAuth account is still connected."))
        store.askToDisconnect("calendar")
        await store.confirmDisconnect()
        XCTAssertEqual(store.sourceErrors["calendar"], "An OAuth account is still connected.")
        XCTAssertEqual(store.settings?.connections.first { $0.id == "calendar" }?.connected, true)
    }

    func testDisconnectCopySaysWhatHeronForgets() {
        let row = ConnectionInfo(id: "email", name: "Email", connected: true, detail: "sam@example.com")
        XCTAssertTrue(SourceCopy.forgetSentence(row).contains("app password"))
        XCTAssertTrue(SourceCopy.forgetSentence(row).contains("forget"))
        XCTAssertTrue(SourceCopy.forgetSentence(ConnectionInfo(id: "documents", name: "Documents", connected: true, detail: "")).contains("forget"))
        XCTAssertTrue(SourceCopy.scanSentence.contains("opens no files"))
        XCTAssertEqual(SourceCopy.connectTitle("computer"), "Scan")
        XCTAssertEqual(SourceCopy.connectTitle("email"), "Connect…")
    }

    // MARK: Sign in with Google or Microsoft

    /// Opened URLs, captured on the main actor.
    @MainActor private final class Opened { var urls: [URL] = []; var canOpen = true }

    private func signInStore(_ client: PreviewCoreClient, calendar: FakeCalendar = FakeCalendar()) async -> SettingsStore {
        let store = SettingsStore(client: client, localCalendar: calendar, sleep: { _ in await Task.yield() })
        await store.load()
        return store
    }

    func testSignInOpensTheConsentPagePollsUntilConnectedThenReadsTheAccount() async throws {
        let client = try await makeClient()
        client.completeSignIn(afterPolls: 3, address: "sam@gmail.com")
        let store = await signInStore(client)
        XCTAssertFalse(store.account(.google).connected)
        let opened = Opened()
        await store.beginSignIn(.google) { opened.urls.append($0); return opened.canOpen }.value
        XCTAssertEqual(opened.urls, [URL(string: "https://accounts.example.com/consent?provider=google")!], "the consent page went to the system browser")
        XCTAssertTrue(store.account(.google).connected)
        XCTAssertEqual(store.account(.google).account, "sam@gmail.com")
        XCTAssertEqual(client.oauthSyncs, [.google], "the first read was triggered once connected")
        XCTAssertNil(store.signingIn)
        XCTAssertTrue(store.syncingAccounts.isEmpty)
        XCTAssertNil(store.sourceErrors["google"])
        XCTAssertFalse(store.account(.microsoft).connected, "one provider's sign-in doesn't touch the other")
    }

    func testWhileWaitingTheStoreSaysSoAndCancelStopsWithoutConnecting() async throws {
        let client = try await makeClient()
        client.completeSignIn(afterPolls: nil)   // the user never finishes
        let store = SettingsStore(client: client, localCalendar: FakeCalendar(), sleep: { _ in try await Task.sleep(for: .seconds(60)) })
        await store.load()
        let task = store.beginSignIn(.google) { _ in true }
        for _ in 0..<200 where store.signingIn == nil { await Task.yield() }
        XCTAssertEqual(store.signingIn, .google, "waiting for the browser")
        store.cancelSignIn()
        XCTAssertNil(store.signingIn)
        await task.value
        XCTAssertFalse(store.account(.google).connected)
        XCTAssertTrue(client.oauthSyncs.isEmpty, "nothing is read after a cancel")
        XCTAssertNil(store.sourceErrors["google"], "cancelling is not an error")
    }

    func testSignInThatIsNotFinishedInTimeSaysSoAndStopsWaiting() async throws {
        let client = try await makeClient()
        client.completeSignIn(afterPolls: nil)
        let store = await signInStore(client)
        store.pollInterval = .seconds(2)
        store.signInTimeout = .seconds(10)
        await store.beginSignIn(.microsoft) { _ in true }.value
        XCTAssertEqual(store.sourceErrors["microsoft"], SourceCopy.signInTimedOut)
        XCTAssertNil(store.signingIn)
    }

    func testAProviderThisBuildCannotSignInToIsOffAndNeverStartsASignIn() async throws {
        let client = try await makeClient()
        client.setOAuthConfigured(.google, false)
        let store = await signInStore(client)
        XCTAssertFalse(store.account(.google).clientConfigured)
        XCTAssertTrue(store.account(.microsoft).clientConfigured, "the other provider is unaffected")
        let opened = Opened()
        await store.beginSignIn(.google) { opened.urls.append($0); return true }.value
        XCTAssertTrue(opened.urls.isEmpty, "nothing opens")
        XCTAssertEqual(store.sourceErrors["google"], "Google sign-in isn't set up in this build yet.")
        XCTAssertNil(store.signingIn)
    }

    func testAFailedStartAndABrowserThatWontOpenShowAMessage() async throws {
        let client = try await makeClient()
        let store = await signInStore(client)
        client.failNextOAuthStart(with: .failed("Too many unfinished sign-ins. Try again later."))
        await store.beginSignIn(.google) { _ in true }.value
        XCTAssertEqual(store.sourceErrors["google"], "Too many unfinished sign-ins. Try again later.")
        await store.beginSignIn(.google) { _ in false }.value
        XCTAssertEqual(store.sourceErrors["google"], SourceCopy.browserWouldNotOpen)
        XCTAssertNil(store.signingIn)
    }

    func testAFirstReadThatFailsKeepsTheAccountConnectedAndSaysWhat() async throws {
        let client = try await makeClient()
        client.completeSignIn(afterPolls: 1)
        client.failNextOAuthSync(with: .failed("Read-only account sync failed (HTTP 403)."))
        let store = await signInStore(client)
        await store.beginSignIn(.google) { _ in true }.value
        XCTAssertTrue(store.account(.google).connected, "the sign-in itself worked")
        XCTAssertTrue(store.sourceErrors["google"]?.contains("Read-only account sync failed (HTTP 403).") ?? false)
    }

    func testDisconnectingAnAccountAsksFirstThenSignsOut() async throws {
        let client = try await makeClient()
        client.completeSignIn(afterPolls: 1)
        let store = await signInStore(client)
        await store.beginSignIn(.google) { _ in true }.value
        store.askToDisconnect(account: .google)
        XCTAssertEqual(store.pendingDisconnect?.id, "google")
        XCTAssertTrue(SourceCopy.forgetSentence(try XCTUnwrap(store.pendingDisconnect)).contains("sign out of this account"))
        XCTAssertTrue(store.account(.google).connected, "asking changes nothing")
        await store.confirmDisconnect()
        XCTAssertFalse(store.account(.google).connected)
        XCTAssertNil(store.pendingDisconnect)
        store.askToDisconnect(account: .microsoft)
        XCTAssertNil(store.pendingDisconnect, "an account that isn't connected has nothing to disconnect")
    }

    // MARK: The Mac's Calendar (EventKit)

    func testMacCalendarAsksMacOSThenSendsTheEventsToTheCore() async throws {
        let client = try await makeClient()
        let calendar = FakeCalendar(status: .notDetermined, afterPrompt: .granted,
                                    events: [LocalCalendarEvent(uid: "e1", title: "Dentist", start: 1_795_100_000), LocalCalendarEvent(uid: "e2", title: "Standup", start: 1_795_200_000)])
        let store = await signInStore(client, calendar: calendar)
        let ok = await store.connectLocalCalendar()
        XCTAssertTrue(ok)
        XCTAssertEqual(calendar.prompts, 1, "the macOS prompt was shown once")
        XCTAssertEqual(client.sourceRequests.last, .localCalendar(events: calendar.eventList))
        XCTAssertEqual(store.settings?.connections.first { $0.id == "calendar_local" }?.connected, true)
        XCTAssertFalse(store.localCalendarDenied)
    }

    func testMacCalendarDeniedExplainsHowToTurnItOnInSystemSettings() async throws {
        let client = try await makeClient()
        let calendar = FakeCalendar(status: .denied, afterPrompt: .denied)
        let store = await signInStore(client, calendar: calendar)
        let ok = await store.connectLocalCalendar()
        XCTAssertFalse(ok)
        XCTAssertEqual(calendar.prompts, 0, "macOS won't ask twice, so Heron doesn't try")
        XCTAssertTrue(store.localCalendarDenied)
        let message = try XCTUnwrap(store.sourceErrors["calendar_local"])
        XCTAssertTrue(message.contains("System Settings"))
        XCTAssertTrue(message.contains("Calendars"))
        XCTAssertTrue(client.sourceRequests.isEmpty, "nothing was sent")
        XCTAssertTrue(SourceCopy.privacyCalendarsURL.hasSuffix("Privacy_Calendars"))
        XCTAssertFalse(store.isBusy(source: "calendar_local"))
    }

    func testRefusingTheMacOSPromptIsTheSameAsDenied() async throws {
        let client = try await makeClient()
        let calendar = FakeCalendar(status: .notDetermined, afterPrompt: .denied)
        let store = await signInStore(client, calendar: calendar)
        let ok = await store.connectLocalCalendar()
        XCTAssertFalse(ok)
        XCTAssertEqual(calendar.prompts, 1)
        XCTAssertTrue(store.localCalendarDenied)
    }

    func testAConnectedMacCalendarIsReadAgainQuietlyAndNeverPromptsOnItsOwn() async throws {
        let client = try await makeClient()
        let calendar = FakeCalendar(status: .granted, afterPrompt: .granted, events: [LocalCalendarEvent(uid: "e1", title: "Dentist", start: 1_795_100_000)])
        let store = await signInStore(client, calendar: calendar)
        await store.refreshLocalCalendarIfConnected()
        XCTAssertTrue(client.sourceRequests.isEmpty, "not connected, so nothing is read")
        _ = await store.connectLocalCalendar()
        calendar.eventList.append(LocalCalendarEvent(uid: "e2", title: "Standup", start: 1_795_200_000))
        await store.refreshLocalCalendarIfConnected()
        XCTAssertEqual(client.sourceRequests.last, .localCalendar(events: calendar.eventList), "the new event went along")
        // Access was taken away in System Settings: the refresh does nothing and doesn't prompt.
        calendar.status = .denied
        let sent = client.sourceRequests.count
        await store.refreshLocalCalendarIfConnected()
        XCTAssertEqual(client.sourceRequests.count, sent)
        XCTAssertEqual(calendar.prompts, 0)
    }

    // MARK: The Mac's Mail (Apple Events)

    private func mailStore(_ client: PreviewCoreClient, mail: FakeMail) async -> SettingsStore {
        let store = SettingsStore(client: client, localCalendar: FakeCalendar(), localMail: mail, sleep: { _ in await Task.yield() })
        await store.load()
        return store
    }

    private func sampleMail(_ id: String, account: String = "iCloud", date: Int = Int(Date().timeIntervalSince1970)) -> LocalMailMessage {
        LocalMailMessage(id: id, from: "Dana <dana@example.com>", subject: "Invoice", date: date, account: account, body: "Please pay.")
    }

    func testMacMailSendsTheMessagesToTheCoreAndMarksTheRowConnected() async throws {
        let client = try await makeClient()
        let mail = FakeMail(result: .success([sampleMail("<a@x>"), sampleMail("<b@x>", account: "Outlook")]))
        let store = await mailStore(client, mail: mail)
        let ok = await store.connectLocalMail()
        XCTAssertTrue(ok)
        XCTAssertEqual(client.sourceRequests.last, .localMail(messages: try mail.result.get()))
        let row = try XCTUnwrap(store.settings?.connections.first { $0.id == "mail_local" })
        XCTAssertEqual([row.connected, row.detail == "2 messages indexed"], [true, true])
        XCTAssertNil(store.localMailProblem)
        XCTAssertFalse(store.isBusy(source: "mail_local"))
    }

    func testMacMailPermissionDeniedShowsHowToTurnItOnAndOffersTheAutomationPane() async throws {
        let client = try await makeClient()
        let store = await mailStore(client, mail: FakeMail(result: .failure(.notAuthorized)))
        let ok = await store.connectLocalMail()
        XCTAssertFalse(ok)
        XCTAssertEqual(store.localMailProblem, .automationDenied)
        let message = try XCTUnwrap(store.sourceErrors["mail_local"])
        XCTAssertTrue(message.contains("System Settings") && message.contains("Automation") && message.contains("Mail"), message)
        XCTAssertTrue(client.sourceRequests.isEmpty, "nothing was sent")
        XCTAssertTrue(SourceCopy.privacyAutomationURL.hasSuffix("Privacy_Automation"))
        XCTAssertFalse(store.isBusy(source: "mail_local"))
        // Asking again after turning it on works, and clears the problem.
        let fixed = FakeMail(result: .success([sampleMail("<a@x>")]))
        let again = await mailStore(client, mail: fixed)
        let retried = await again.connectLocalMail()
        XCTAssertTrue(retried)
        XCTAssertNil(again.localMailProblem)
    }

    func testMacMailWithNoAccountsSaysHowToAddOneAndOffersInternetAccounts() async throws {
        let client = try await makeClient()
        let store = await mailStore(client, mail: FakeMail(result: .failure(.noAccounts)))
        let ok = await store.connectLocalMail()
        XCTAssertFalse(ok)
        XCTAssertEqual(store.localMailProblem, .noAccounts)
        XCTAssertTrue(try XCTUnwrap(store.sourceErrors["mail_local"]).contains("Internet Accounts"))
        XCTAssertTrue(client.sourceRequests.isEmpty)
        XCTAssertTrue(SourceCopy.internetAccountsURL.contains("Internet-Accounts"))
    }

    // The silent-failure paths: every one ends with the row connected or a visible sentence.

    func testAnEmptyMailIsConnectedWithAVisibleNoteNotASilentZero() async throws {
        let client = try await makeClient()
        let store = await mailStore(client, mail: FakeMail(result: .success([])))
        let ok = await store.connectLocalMail()
        XCTAssertTrue(ok, "it did connect")
        XCTAssertEqual(store.settings?.connections.first { $0.id == "mail_local" }?.connected, true)
        XCTAssertEqual(store.sourceErrors["mail_local"], SourceCopy.mailWasEmpty)
        XCTAssertTrue(SourceCopy.mailWasEmpty.contains("no inbox messages from the last 14 days") && SourceCopy.mailWasEmpty.contains("finished downloading"))
    }

    func testAConnectThatWorkedButCouldNotRefreshTheRowsSaysSo() async throws {
        let client = try await makeClient()
        let store = await mailStore(client, mail: FakeMail(result: .success([sampleMail("<a@x>")])))
        client.failNextSettingsRead(with: .failed("offline"))
        let ok = await store.connectLocalMail()
        XCTAssertTrue(ok)
        XCTAssertEqual(store.sourceErrors["mail_local"], SourceCopy.connectedButCouldNotRefresh, "the list still says Not connected, so the row explains")
        XCTAssertEqual(client.sourceRequests.count, 1)
    }

    func testACancelledMailReadSaysSoAndIsNotBusyAfterwards() async throws {
        let client = try await makeClient()
        let mail = FakeMail(result: .success([]))
        mail.cancelled = true
        let store = await mailStore(client, mail: mail)
        let ok = await store.connectLocalMail()
        XCTAssertFalse(ok)
        XCTAssertEqual(store.sourceErrors["mail_local"], SourceCopy.cancelledBeforeFinishing)
        XCTAssertFalse(store.isBusy(source: "mail_local"))
        XCTAssertTrue(client.sourceRequests.isEmpty)
    }

    func testAnyMailFailureTextOnTheRowIsCapped() async throws {
        let client = try await makeClient()
        let store = await mailStore(client, mail: FakeMail(result: .failure(.failed(String(repeating: "x", count: 10_000)))))
        _ = await store.connectLocalMail()
        XCTAssertEqual(store.sourceErrors["mail_local"]?.count, SourceCopy.maxErrorCharacters)
    }

    func testMacMailThatCannotBeOpenedOrFailsIsExplainedWithoutASettingsButton() async throws {
        let client = try await makeClient()
        let notRunning = await mailStore(client, mail: FakeMail(result: .failure(.notRunning)))
        _ = await notRunning.connectLocalMail()
        XCTAssertEqual(notRunning.sourceErrors["mail_local"], SourceCopy.mailWouldNotOpen)
        XCTAssertNil(notRunning.localMailProblem)
        let broken = await mailStore(client, mail: FakeMail(result: .failure(.failed("Mail answered with error 7."))))
        _ = await broken.connectLocalMail()
        XCTAssertEqual(broken.sourceErrors["mail_local"], "Mail answered with error 7.", "a short message passes through unchanged")
    }

    func testMacMailTakesItsTimeOffTheMainThreadAndShowsBusyUntilMailAnswers() async throws {
        let client = try await makeClient()
        // Apple Events opens Mail first, so the answer can be slow: the store stays responsive and busy meanwhile.
        let mail = FakeMail(result: .success([sampleMail("<a@x>")]), delay: .milliseconds(150))
        let store = await mailStore(client, mail: mail)
        let reading = Task { await store.connectLocalMail() }
        for _ in 0..<500 where !store.isBusy(source: "mail_local") { await Task.yield() }
        XCTAssertTrue(store.isBusy(source: "mail_local"), "the row shows the wait")
        let second = await store.connectLocalMail()
        XCTAssertFalse(second, "a second click while Mail is answering does nothing")
        let ok = await reading.value
        XCTAssertTrue(ok)
        XCTAssertEqual(mail.calls, 1)
        XCTAssertFalse(store.isBusy(source: "mail_local"))
    }

    func testAConnectedMacMailIsReadAgainQuietlyOnlyWhileMailIsOpen() async throws {
        let client = try await makeClient()
        let mail = FakeMail(result: .success([sampleMail("<a@x>")]))
        let store = await mailStore(client, mail: mail)
        await store.refreshLocalMailIfConnected()
        XCTAssertTrue(client.sourceRequests.isEmpty, "not connected, so nothing is read")
        _ = await store.connectLocalMail()
        mail.result = .success([sampleMail("<a@x>"), sampleMail("<c@x>")])
        mail.running = false
        let reads = mail.calls
        await store.refreshLocalMailIfConnected()
        XCTAssertEqual(mail.calls, reads, "a closed Mail is left closed: Apple Events would open it")
        mail.running = true
        await store.refreshLocalMailIfConnected()
        XCTAssertEqual(client.sourceRequests.last, .localMail(messages: try mail.result.get()), "the new message went along")
        // Permission taken away: the refresh does nothing visible.
        mail.result = .failure(.notAuthorized)
        let sent = client.sourceRequests.count
        await store.refreshLocalMailIfConnected()
        XCTAssertEqual(client.sourceRequests.count, sent)
        XCTAssertNil(store.sourceErrors["mail_local"])
    }

    func testDisconnectingMacMailAsksFirstThenForgetsItsMessages() async throws {
        let client = try await makeClient()
        let store = await mailStore(client, mail: FakeMail(result: .success([sampleMail("<a@x>")])))
        _ = await store.connectLocalMail()
        store.askToDisconnect("mail_local")
        XCTAssertEqual(store.pendingDisconnect?.id, "mail_local")
        XCTAssertTrue(SourceCopy.forgetSentence(try XCTUnwrap(store.pendingDisconnect)).contains("forget the messages"))
        XCTAssertEqual(store.settings?.connections.first { $0.id == "mail_local" }?.connected, true, "nothing changes before the confirm")
        await store.confirmDisconnect()
        XCTAssertEqual(store.settings?.connections.first { $0.id == "mail_local" }?.connected, false)
    }

    // MARK: Run setup again

    func testRunSetupAgainShowsSetupWithoutUnsettingOnboardedAndKeepsAcceptedRisks() async throws {
        let client = try await makeClient()   // risk notice already accepted, onboarded by the preview sample
        try await client.setOnboarded()
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!),
                             userFirstName: "Sam", now: { self.now })
        await model.bootstrap()
        model.stopEvents()
        XCTAssertFalse(model.needsFirstRun)
        XCTAssertFalse(model.showsSetup)
        await model.runSetupAgain()
        XCTAssertTrue(model.showsSetup)
        XCTAssertEqual(model.firstRun.step, .risks)
        XCTAssertTrue(model.firstRun.accepted, "the risks show as already accepted")
        XCTAssertTrue(model.firstRun.canAccept)
        let stillOnboarded = try await client.state().onboarded
        XCTAssertTrue(stillOnboarded, "the core's onboarded flag is left alone")
        await model.firstRun.accept()
        XCTAssertEqual(model.firstRun.step, .model)
        await model.firstRun.continueFromModel()
        XCTAssertEqual(model.firstRun.step, .sources)
        model.firstRun.continueFromSources()
        await model.finishFirstRun()
        XCTAssertFalse(model.showsSetup)
        XCTAssertFalse(model.isRerunningSetup)
        XCTAssertEqual(model.selection, .today)
    }

    func testClosingSetupAgainLeavesEverythingAsItWas() async throws {
        let client = try await makeClient()
        try await client.setOnboarded()
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!),
                             userFirstName: "Sam", now: { self.now })
        await model.bootstrap()
        model.stopEvents()
        await model.runSetupAgain()
        model.firstRun.step = .sources
        model.cancelSetupAgain()
        XCTAssertFalse(model.showsSetup)
        XCTAssertEqual(model.firstRun.step, .risks)
        XCTAssertFalse(model.needsFirstRun)
    }

    // MARK: Sidebar footer model name

    func testSidebarFooterNamesTheProviderNotTheConfiguredModel() async throws {
        let client = try await makeClient()
        client.setModel("anthropic/claude-sonnet-5.5", label: "Claude Code")   // [llm] provider = "claude-cli", llm.model still set
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!),
                             userFirstName: "Sam", now: { self.now })
        await model.refreshState()
        XCTAssertEqual(model.state?.model, "anthropic/claude-sonnet-5.5")
        XCTAssertEqual(model.footerModelName, "Claude Code", "llm.model is not what a CLI provider runs")
        client.setModel("gpt-4o", label: nil, servesLabel: false)   // a core from before model_label
        await model.refreshState()
        XCTAssertEqual(model.footerModelName, "gpt-4o")
        let bare = AppModel(client: client, preferences: DisplayPreferences(defaults: UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!), now: { self.now })
        XCTAssertEqual(bare.footerModelName, "No model", "before the first state")
    }

    // MARK: Settings: the Never list (heron-v1.md item 35h)

    func testNeverListLoadsWithSettings() async throws {
        let store = SettingsStore(client: try await makeClient())
        await store.load()
        XCTAssertEqual(store.nevers.map(\.rule), ["web_fetch:*", "shell:*sudo*", "send_email:*@competitor.example"])
        XCTAssertNil(store.neverError)
    }

    func testRemoveAsksFirstAndChangesNothingUntilConfirmed() async throws {
        let client = try await makeClient()
        let store = SettingsStore(client: client)
        await store.load()
        store.askToRemoveNever(store.nevers[1])
        XCTAssertEqual(store.pendingNeverRemoval?.rule, "shell:*sudo*")
        let held = try await client.nevers()
        XCTAssertEqual(held.count, 3, "asking removes nothing")
        store.cancelNeverRemoval()
        XCTAssertNil(store.pendingNeverRemoval)
        await store.confirmNeverRemoval()
        let stillHeld = try await client.nevers()
        XCTAssertEqual(stillHeld.count, 3, "confirming with nothing pending removes nothing")
    }

    func testConfirmedRemovalDeletesThenRefreshesFromTheCore() async throws {
        let client = try await makeClient()
        let store = SettingsStore(client: client)
        await store.load()
        // Another core process already removed a rule; the refresh must show that too, not only this removal.
        try await client.removeNever(rule: "web_fetch:*")
        store.askToRemoveNever(store.nevers[1])
        await store.confirmNeverRemoval()
        XCTAssertEqual(store.nevers.map(\.rule), ["send_email:*@competitor.example"])
        XCTAssertNil(store.pendingNeverRemoval)
        XCTAssertNil(store.neverError)
        XCTAssertFalse(store.isRemovingNever)
    }

    func testFailedRemovalShowsTheErrorInlineAndKeepsTheRule() async throws {
        let client = try await makeClient()
        let store = SettingsStore(client: client)
        await store.load()
        client.failNextNeverRemoval(with: .failed("Heron couldn't update config.toml, so the rule is still in force."))
        store.askToRemoveNever(store.nevers[0])
        await store.confirmNeverRemoval()
        XCTAssertEqual(store.neverError, "Heron couldn't update config.toml, so the rule is still in force.")
        XCTAssertEqual(store.nevers.count, 3)
        XCTAssertFalse(store.isRemovingNever)
        // The next removal clears the error.
        store.askToRemoveNever(store.nevers[0])
        await store.confirmNeverRemoval()
        XCTAssertNil(store.neverError)
        XCTAssertEqual(store.nevers.count, 2)
    }

    func testRemovingARuleAlreadyGoneSaysSoAndRefreshes() async throws {
        let client = try await makeClient()
        let store = SettingsStore(client: client)
        await store.load()
        let stale = store.nevers[0]
        try await client.removeNever(rule: stale.rule)
        store.askToRemoveNever(stale)
        await store.confirmNeverRemoval()
        XCTAssertEqual(store.neverError, "That item no longer exists.")
        XCTAssertFalse(store.nevers.contains(stale), "the list was refreshed")
    }

    func testNeverCopyNamesWhatHeronWillBeAllowedToDo() {
        func sentence(_ rule: String, _ tool: String) -> String { NeverCopy.allowedSentence(NeverRule(rule: rule, tool: tool, inFile: true)) }
        XCTAssertEqual(sentence("web_fetch:*", "web_fetch"), "Heron will be allowed to fetch web pages again.")
        XCTAssertEqual(sentence("shell:*sudo*", "shell"), "Heron will be allowed to run commands where it matches “*sudo*”.")
        XCTAssertEqual(sentence("*", "*"), "Heron will be allowed to use any tool again.")
        XCTAssertEqual(sentence("*sudo*", "*"), "Heron will be allowed to do what the rule “*sudo*” refused.")
        XCTAssertEqual(sentence("frobnicate:*", "frobnicate"), "Heron will be allowed to use frobnicate again.")
    }

    func testARuleOutsideConfigTomlSaysItIsKeptByHeronOnly() {
        XCTAssertEqual(NeverCopy.whereItLives(NeverRule(rule: "a:*", tool: "a", inFile: true)), "In config.toml")
        XCTAssertTrue(NeverCopy.whereItLives(NeverRule(rule: "a:*", tool: "a", inFile: false)).hasPrefix("Kept by Heron only"))
    }

    func testSettingsShowsSandboxGrantsProvidersAndNotices() async throws {
        let client = try await makeClient()
        let store = SettingsStore(client: client)
        await store.load()
        XCTAssertTrue(store.sandboxOn)
        XCTAssertEqual(store.grants.count, 2)
        XCTAssertEqual(store.ruleNotices.first?.rule, "read_file:*")
        XCTAssertEqual(store.providers.map(\.rowState), [.signedIn, .signedOut, .untested, .notInstalled])
        XCTAssertEqual(store.usage?.monthTokens, 412_000)
        await store.revoke(store.grants[0])
        XCTAssertEqual(store.grants.map(\.rule), ["shell:git *"])
        await store.setMode(.auto)
        XCTAssertEqual(store.mode, .auto)
        await store.setUnattended(true)
        XCTAssertEqual(store.settings?.allowSubscriptionUnattended, true)
        let untested = try XCTUnwrap(store.providers.first { $0.rowState == .untested })
        await store.acceptVersion(untested)
        XCTAssertEqual(store.providers.first { $0.id == untested.id }?.rowState, .signedIn)
        client.setSandbox("none")
        await store.load()
        XCTAssertFalse(store.sandboxOn)
    }

    func testProviderRowTextFollowsTheVisualDoc() async throws {
        let client = try await makeClient()
        let list = try await client.providers()
        XCTAssertEqual(list[0].detailText, "Version 2.1.4 · tested")
        XCTAssertEqual(list[1].detailText, "Codex is signed out. Run `codex login` in Terminal, then check again.")
        XCTAssertEqual(list[2].detailText, "Version 0.9.2 hasn't been tested with Heron (tested: 0.6 to 0.8).")
        XCTAssertEqual(list[3].detailText, "Not installed on this Mac.")
        XCTAssertEqual(list.map(\.isSelectable), [true, false, true, false])
    }

    // MARK: Persona

    func testPersonaValidatesAndSaves() async throws {
        let client = try await makeClient()
        let store = PersonaStore(client: client)
        await store.load()
        XCTAssertEqual(store.name, "Heron")
        XCTAssertFalse(store.isDirty)
        XCTAssertFalse(store.canSave)
        store.draftName = "Wren"
        XCTAssertTrue(store.canSave)
        store.draftName = "   "
        XCTAssertFalse(store.canSave, "a blank name can't be saved")
        store.draftName = String(repeating: "x", count: 41)
        XCTAssertFalse(store.canSave, "names are at most 40 characters")
        store.draftName = "Wren"
        store.draftText = String(repeating: "a", count: store.maxCharacters + 1)
        XCTAssertFalse(store.canSave)
        XCTAssertEqual(store.charactersLeft, -1)
        store.draftText = "Dry and quick."
        let saved = await store.save()
        XCTAssertTrue(saved)
        XCTAssertEqual(store.persona?.name, "Wren")
        XCTAssertFalse(store.isDirty)
    }

    // MARK: First run

    func testFirstRunShowsSandboxStatusAndAcceptsTheNotice() async throws {
        let client = try await makeClient(accepted: false)
        let store = FirstRunStore(client: client)
        await store.load()
        XCTAssertEqual(store.notice?.version, 3)
        XCTAssertTrue(store.notice?.text.contains("C35-8") ?? false)
        XCTAssertTrue(store.notice?.text.contains("R-1") ?? false)
        XCTAssertTrue(store.sandboxOn)
        XCTAssertFalse(store.canAccept, "the box is off by default")
        await store.accept()
        XCTAssertEqual(store.step, .risks, "nothing happens until the box is ticked")
        store.accepted = true
        XCTAssertTrue(store.canAccept)
        await store.accept()
        XCTAssertEqual(store.step, .model)
        XCTAssertEqual(store.providers.count, 4)
        XCTAssertEqual(store.recommendedID, "claude-cli")
        XCTAssertEqual(store.selectedProviderID, "claude-cli")
        let state = try await client.state()
        XCTAssertTrue(state.riskNoticeAccepted)
    }

    func test_experimentalProviderIsNeverRecommendedOrCalledTested() {
        func p(_ id: String, experimental: Bool, active: Bool = false) -> ProviderStatus {
            ProviderStatus(id: id, displayName: id, installed: true, version: "2.1.4", testedRange: ">=2.1.4 <3.0.0",
                           inRange: true, loggedIn: true, experimental: experimental, versionAccepted: false,
                           loginCommand: "\(id) login", authKind: "cli", isActive: active)
        }
        let opencode = p("opencode-cli", experimental: true, active: true)
        let claude = p("claude-cli", experimental: false)
        XCTAssertEqual(FirstRunStore.recommendedID(in: [opencode, claude]), "claude-cli")
        XCTAssertNil(FirstRunStore.recommendedID(in: [opencode]), "an experimental provider alone gets no tag")
        XCTAssertEqual(opencode.detailText, "Version 2.1.4 · experimental, not tested yet")
        XCTAssertFalse(opencode.detailText.contains("tested range"))
        XCTAssertEqual(claude.detailText, "Version 2.1.4 · tested")
    }

    func testFirstRunSandboxOffShowsTheOffCard() async throws {
        let client = try await makeClient(accepted: false)
        client.setSandbox("none")
        let store = FirstRunStore(client: client)
        await store.load()
        XCTAssertFalse(store.sandboxOn, "the notice can still be accepted")
        store.accepted = true
        await store.accept()
        XCTAssertEqual(store.step, .model)
    }

    func testFirstRunHandlesAChangedRiskRegister() async throws {
        let client = try await makeClient(accepted: false)
        let store = FirstRunStore(client: client)
        await store.load()
        store.accepted = true
        client.simulateRiskNoticeChange()   // the register changed while the screen was open
        await store.accept()
        XCTAssertEqual(store.step, .risks)
        XCTAssertFalse(store.accepted, "the box is unticked")
        XCTAssertTrue(store.noticeUpdated)
        XCTAssertEqual(store.notice?.version, 4, "the new text is loaded")
        store.accepted = true
        await store.accept()
        XCTAssertEqual(store.step, .model)
        XCTAssertFalse(store.noticeUpdated)
    }

    func testFirstRunModelStepAndFinish() async throws {
        let client = try await makeClient(accepted: false)
        let store = FirstRunStore(client: client)
        await store.load()
        store.accepted = true
        await store.accept()
        store.selectedProviderID = "claude-cli"
        await store.continueFromModel()
        XCTAssertEqual(store.step, .sources, "the sources step sits between the model and the name")
        store.continueFromSources()
        XCTAssertEqual(store.step, .personalize)
        let persona = PersonaStore(client: client)
        await persona.load()
        persona.draftName = "Wren"
        await store.finish(persona: persona)
        XCTAssertTrue(store.isFinished)
        let state = try await client.state()
        XCTAssertTrue(state.onboarded)
        XCTAssertEqual(state.name, "Wren")
    }

    func testUseAnAPIKeyInsteadSkipsTheModelListButStillOffersSources() async throws {
        let store = FirstRunStore(client: try await makeClient(accepted: false))
        await store.useAPIKeyInstead()
        XCTAssertEqual(store.step, .sources)
        XCTAssertTrue(store.wantsAPIKey)
    }

    func testFirstRunStepsRunInOrderWithSourcesBetweenModelAndName() {
        XCTAssertEqual(FirstRunStore.Step.allCases.map(\.title),
                       ["Read the risks", "Connect a model", "Connect your sources", "Make it yours"])
        XCTAssertEqual(FirstRunStore.Step.allCases.map(\.rawValue), [0, 1, 2, 3])
    }

    func testFirstRunCanFinishWithoutConnectingAnySource() async throws {
        let client = try await makeClient(accepted: false)
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!),
                             userFirstName: "Sam", now: { self.now })
        await model.bootstrap()
        model.stopEvents()
        model.firstRun.accepted = true
        await model.firstRun.accept()
        await model.firstRun.continueFromModel()
        model.firstRun.continueFromSources()   // "Skip for now"
        await model.finishFirstRun()
        XCTAssertFalse(model.needsFirstRun)
        XCTAssertTrue(client.sourceRequests.isEmpty, "skipping connects nothing")
    }

    // MARK: App model

    func testAppModelRoutesFirstRunThenMainAndEvents() async throws {
        let client = try await makeClient(accepted: false)
        let defaults = UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: defaults), userFirstName: "Sam", now: { self.now })
        await model.bootstrap()
        XCTAssertTrue(model.needsFirstRun)
        model.stopEvents()
        model.firstRun.accepted = true
        await model.firstRun.accept()
        await model.firstRun.continueFromModel()
        await model.finishFirstRun()
        XCTAssertFalse(model.needsFirstRun)
        XCTAssertEqual(model.selection, .today)
        XCTAssertEqual(model.footerLine, "Sandbox on · Ask first")
        XCTAssertEqual(model.today.summary?.events.count, 3)

        // An approval for another conversation goes to the Inbox feed, not to a sheet.
        let request = ApprovalRequest(id: "z1", tool: "write_file", summary: "Create a file", risk: "write", detail: "d", alwaysRule: nil,
                                      canAlways: true, canSession: true, tainted: false, outsideSource: nil, reason: nil, sessionID: "other")
        model.route(CoreEvent(sessionID: "other", runID: "r", kind: .approval(request)))
        XCTAssertNil(model.chat.liveApproval)
        XCTAssertEqual(model.approvals.live.map(\.id), ["z1"])
        XCTAssertEqual(model.waitingCount, 2, "the live item and the parked one")
        XCTAssertEqual(model.avatar.state, .awaitingApproval)
        model.route(CoreEvent(sessionID: "other", runID: "r", kind: .approvalResolved(id: "z1", decision: "once")))
        XCTAssertTrue(model.approvals.live.isEmpty)
    }

    func testPrepMyDayStartsAConversationWithTheApprovedPrompt() async throws {
        let client = try await makeClient()
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!),
                             userFirstName: "Sam", now: { self.now })
        await model.prepMyDay()
        XCTAssertEqual(model.selection, .chat)
        XCTAssertEqual(model.chat.messages.first?.text, TodayStore.prepPrompt)
        XCTAssertEqual(model.chat.sessions.first?.title, "Prep · Wed 18 Nov")
        await model.chat.stop()
    }

    func testAnotherHeronBannerFollowsTheEndpointLine() async throws {
        let owner = AppModel(client: try await makeClient(backgroundOwner: true))
        let other = AppModel(client: try await makeClient(backgroundOwner: false))
        // No banner until the endpoint is known: before bootstrap neither model shows it.
        XCTAssertFalse(owner.showsAnotherHeronBanner)
        XCTAssertFalse(other.showsAnotherHeronBanner, "the endpoint line isn't known before bootstrap")
        await owner.bootstrap()
        await other.bootstrap()
        owner.stopEvents()
        other.stopEvents()
        XCTAssertFalse(owner.showsAnotherHeronBanner, "this app owns the background core")
        XCTAssertTrue(other.showsAnotherHeronBanner)
        other.dismissedAnotherHeronBanner = true
        XCTAssertFalse(other.showsAnotherHeronBanner)
    }

    func testPauseAndModeGoThroughTheClient() async throws {
        let client = try await makeClient()
        let model = AppModel(client: client)
        await model.setPaused(true)
        XCTAssertTrue(model.paused)
        await model.setPaused(false)
        XCTAssertFalse(model.paused)
        await model.setMode(.readonly)
        XCTAssertEqual(model.state?.mode, .readonly)
        XCTAssertEqual(model.footerLine, "Sandbox on · Read only")
    }

    func testReviewingAParkedItemOpensTheInboxOnIt() async throws {
        let model = AppModel(client: try await makeClient())
        model.review(parked: 1)
        XCTAssertEqual(model.selection, .inbox)
        XCTAssertEqual(model.inbox.selection, .parked(1))
        XCTAssertEqual(Destination.allCases.map(\.shortcutDigit), [1, 2, 3, 4, 5, 6, 7])
        XCTAssertEqual(Destination.inbox.symbol, "tray")
    }

    // MARK: Display preferences

    func testDisplayPreferencesPersistUnderTheDocumentedKeys() {
        let suite = "heron.tests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        let p = DisplayPreferences(defaults: defaults)
        XCTAssertEqual(p.avatarStyle, .ripple)
        XCTAssertEqual(p.theme, .dusk)
        XCTAssertEqual(p.accentColor, .system)
        XCTAssertEqual(p.appearance, .system)
        XCTAssertEqual(p.textSize, .m)
        p.avatarStyle = .monogram
        p.theme = .graphite
        p.accentColor = .rose
        XCTAssertEqual(defaults.string(forKey: "avatarStyle"), "monogram")
        XCTAssertEqual(defaults.string(forKey: "theme"), "graphite")
        XCTAssertEqual(defaults.string(forKey: "accentColor"), "rose")
        let again = DisplayPreferences(defaults: defaults)
        XCTAssertEqual(again.avatarStyle, .monogram)
        XCTAssertEqual(again.theme, .graphite)
        XCTAssertEqual(again.accentColor, .rose)
        defaults.set("nonsense", forKey: "theme")
        XCTAssertEqual(DisplayPreferences(defaults: defaults).theme, .dusk, "an unknown stored value falls back to the default")
        XCTAssertTrue(DisplayPreferences.avatarImageURL.path.hasSuffix("Application Support/Heron/avatar.png"))
    }

    func testTextSizeSteps() {
        XCTAssertEqual(TextSizeChoice.m.larger(), .l)
        XCTAssertEqual(TextSizeChoice.xl.larger(), .xl)
        XCTAssertEqual(TextSizeChoice.s.smaller(), .s)
        XCTAssertEqual(TextSizeChoice.l.smaller(), .m)
        XCTAssertEqual(TextSizeChoice.allCases.map(\.scale), TextSizeChoice.allCases.map(\.scale).sorted())
        XCTAssertEqual(TextSizeChoice.m.scale, 1)
    }
}


/// A stand-in for the Mac's Mail app, so no test reaches Apple Events or the Automation prompt.
final class FakeMail: LocalMailAccess, @unchecked Sendable {
    var result: Result<[LocalMailMessage], MailAccessError>
    var running = true
    var cancelled = false
    let delay: Duration?
    private let lock = NSLock()
    private var count = 0
    var calls: Int { lock.withLock { count } }

    init(result: Result<[LocalMailMessage], MailAccessError>, delay: Duration? = nil) { self.result = result; self.delay = delay }

    func isMailRunning() -> Bool { running }
    func messages() async throws -> [LocalMailMessage] {
        lock.withLock { count += 1 }
        if let delay { try await Task.sleep(for: delay) }
        if cancelled { throw CancellationError() }
        return try result.get()
    }
}

/// A stand-in for the Mac's Calendar app, so no test reaches EventKit or the permission prompt.
final class FakeCalendar: LocalCalendarAccess, @unchecked Sendable {
    var status: CalendarAuthorization
    let afterPrompt: CalendarAuthorization
    var eventList: [LocalCalendarEvent]
    private(set) var prompts = 0

    init(status: CalendarAuthorization = .granted, afterPrompt: CalendarAuthorization = .granted, events: [LocalCalendarEvent] = []) {
        self.status = status; self.afterPrompt = afterPrompt; self.eventList = events
    }

    func authorization() -> CalendarAuthorization { status }
    func requestAccess() async -> CalendarAuthorization { prompts += 1; status = afterPrompt; return status }
    func events() async throws -> [LocalCalendarEvent] { eventList }
}
