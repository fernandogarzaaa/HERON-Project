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

    func testUseAnAPIKeyInsteadSkipsToPersonalize() async throws {
        let store = FirstRunStore(client: try await makeClient(accepted: false))
        await store.useAPIKeyInstead()
        XCTAssertEqual(store.step, .personalize)
        XCTAssertTrue(store.wantsAPIKey)
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
        XCTAssertFalse(owner.showsAnotherHeronBanner)
        let other = AppModel(client: try await makeClient(backgroundOwner: false))
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
