import XCTest
import HeronCore
@testable import HeronUI

/// The chat bar after a core restart (a cut-off reply, a stopped core) and the Stop button once the reply is done.
@MainActor
final class ChatRecoveryTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_795_003_200)

    private func makeClient() async throws -> PreviewCoreClient {
        let client = PreviewCoreClient(endpoint: CoreEndpoint(port: 1, token: "t", backgroundOwner: true, version: "test"), now: now)
        try await client.acceptRiskNotice(version: 3)
        return client
    }

    private func makeModel(_ client: PreviewCoreClient) -> AppModel {
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!),
                             userFirstName: "Sam", now: { self.now })
        model.selection = .chat
        return model
    }

    /// Hands `stream`'s events to `chat` until `last` matches (inclusive). With `holdingTheRest`, it then reads on
    /// to the run's `run_end` without handing those events over, and returns them.
    private func feed(_ stream: AsyncStream<CoreEvent>, to chat: ChatStore, through last: (CoreEvent.Kind) -> Bool,
                      holdingTheRest: Bool = false) async -> [CoreEvent] {
        var held: [CoreEvent] = []
        var reached = false
        for await event in stream {
            if reached {
                held.append(event)
                if case .runEnd = event.kind { break }
                continue
            }
            chat.handle(event)
            if last(event.kind) {
                if !holdingTheRest { break }
                reached = true
            }
        }
        return held
    }

    private func isDone(_ k: CoreEvent.Kind) -> Bool { if case .done = k { return true } else { return false } }
    private func isApproval(_ k: CoreEvent.Kind) -> Bool { if case .approval = k { return true } else { return false } }

    // MARK: Bug 2: Stop while Heron learns from the finished reply

    /// The core frees the conversation and sends `done`, then spends a second model call learning from the
    /// exchange before `run_end`. In that gap the reply is complete and the core accepts a new message.
    func testTheReplyIsOverAtDoneNotAtRunEnd() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client, now: { self.now })
        let stream = client.events(since: nil)
        await chat.send("What is on today?")
        let after = await feed(stream, to: chat, through: isDone, holdingTheRest: true)
        XCTAssertEqual(after.map(\.kind), [.runEnd], "run_end was held back, as while the core learns")
        XCTAssertFalse(chat.isRunning, "the Stop button gives way to Send once the reply is done")
        chat.composer = "And tomorrow?"
        XCTAssertTrue(chat.canSend)
        XCTAssertTrue(try XCTUnwrap(chat.messages.last).finished)
    }

    func testAnEarlierRunsLateEndDoesNotEndTheNextReply() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client, now: { self.now })
        let first = client.events(since: nil)
        await chat.send("What is on today?")
        let lateEnd = await feed(first, to: chat, through: isDone, holdingTheRest: true)
        // The next turn waits for an approval, so it is still running when the first run's end arrives.
        let second = client.events(since: nil)
        await chat.send("Draft a reply to Maria")
        _ = await feed(second, to: chat, through: isApproval)
        XCTAssertTrue(chat.isRunning)
        lateEnd.forEach(chat.handle)
        XCTAssertTrue(chat.isRunning, "the first run's run_end belongs to that run only")
        XCTAssertFalse(try XCTUnwrap(chat.messages.last).finished)
        XCTAssertNotNil(chat.liveApproval)
        await chat.stop()
    }

    /// Stop when the core has no running reply for this conversation (`cancelled: false`): the composer must
    /// not stay stuck on a Stop button that does nothing.
    func testStopWhenTheCoreRunsNothingFreesTheComposer() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client, now: { self.now })
        let stream = client.events(since: nil)
        await chat.send("Draft a reply to Maria")
        _ = await feed(stream, to: chat, through: isApproval)
        client.simulateCoreRestart() // the run is gone; this store never hears the resync
        await chat.stop()
        XCTAssertFalse(chat.isRunning)
        XCTAssertNil(chat.liveApproval)
        let reply = try XCTUnwrap(chat.messages.last)
        XCTAssertTrue(reply.finished)
        XCTAssertNil(chat.composerError)
    }

    func testStopEndsAReplyThatIsStillRunning() async throws {
        let client = try await makeClient()
        let model = makeModel(client)
        let stream = client.events(since: nil)
        await model.chat.send("Draft a reply to Maria")
        _ = await feed(stream, to: model.chat, through: isApproval)
        await model.chat.stop()
        for await event in stream { model.chat.handle(event); if case .runEnd = event.kind { break } }
        XCTAssertFalse(model.chat.isRunning)
        XCTAssertNil(model.chat.liveApproval)
        let live = try await client.liveApprovals()
        XCTAssertTrue(live.isEmpty, "the core stopped waiting")
    }

    // MARK: Bug 1: a core restart in the middle of a reply

    func testACoreRestartMidReplyShowsTheReplyWasCutOffAndTheNextTurnWorks() async throws {
        let client = try await makeClient()
        let model = makeModel(client)
        let stream = client.events(since: nil)
        await model.chat.send("Draft a reply to Maria")
        _ = await feed(stream, to: model.chat, through: isApproval)
        client.simulateCoreRestart()
        for await event in stream { if case .resync = event.kind { model.route(event); break } }
        await model.resynchronize()
        XCTAssertFalse(model.chat.isRunning)
        XCTAssertNil(model.chat.liveApproval)
        let cut = try XCTUnwrap(model.chat.messages.last)
        XCTAssertEqual(cut.role, .assistant)
        XCTAssertEqual(cut.error?.title, "This reply was cut off")
        XCTAssertEqual(cut.error?.actions, [.tryAgain])

        let next = client.events(since: nil)
        await model.chat.send("What is on today?")
        XCTAssertNil(model.chat.composerError)
        _ = await feed(next, to: model.chat, through: { if case .runEnd = $0 { return true } else { return false } })
        let reply = try XCTUnwrap(model.chat.messages.last)
        XCTAssertTrue(reply.text.contains("three events"), reply.text)
        XCTAssertNil(reply.error)
    }

    func testOpeningAConversationWhoseReplyWasCutOffSaysSo() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client, now: { self.now })
        let stream = client.events(since: nil)
        await chat.send("Draft a reply to Maria")
        _ = await feed(stream, to: chat, through: isApproval)
        let id = try XCTUnwrap(chat.currentSessionID)
        client.simulateCoreRestart()
        // A new app window: nothing in memory.
        let relaunched = ChatStore(client: client, now: { self.now })
        await relaunched.open(id: id)
        XCTAssertFalse(relaunched.isRunning)
        XCTAssertEqual(relaunched.messages.map(\.role), [.user, .assistant])
        XCTAssertEqual(relaunched.messages.last?.error?.title, "This reply was cut off")
    }

    func testAFinishedConversationOpensWithoutACutOffNote() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client, now: { self.now })
        let stream = client.events(since: nil)
        await chat.send("What is on today?")
        _ = await feed(stream, to: chat, through: { if case .runEnd = $0 { return true } else { return false } })
        let id = try XCTUnwrap(chat.currentSessionID)
        let reopened = ChatStore(client: client, now: { self.now })
        await reopened.open(id: id)
        XCTAssertNil(reopened.messages.last?.error)
    }

    /// The core stopped and could not be started again: the card says so in words and keeps the message.
    /// The details (exit status, log lines) are never put on screen here.
    func testSendingWhileTheCoreIsStoppedSaysSoWithoutTheDetails() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client, now: { self.now })
        client.failSends(with: .coreStopped(details: "Heron's core exited with status 1.\nsecret-log-line"))
        chat.composer = "What is on today?"
        await chat.send()
        let card = try XCTUnwrap(chat.composerError)
        XCTAssertEqual(card.title, "Heron's core isn't running")
        XCTAssertFalse(card.title.contains("secret-log-line") || card.text.contains("secret-log-line"))
        XCTAssertEqual(card.actions, [.tryAgain])
        XCTAssertEqual(chat.composer, "What is on today?", "the message is kept in the box")
        XCTAssertTrue(chat.messages.isEmpty)
        XCTAssertEqual(describe(CoreError.coreStopped(details: "secret-log-line")), "Heron's core isn't running.")
    }

    /// "Try again" on a card about the box sends what is kept in the box, not the thread's last message.
    func testTryAgainSendsTheMessageKeptInTheBox() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client, now: { self.now })
        let stream = client.events(since: nil)
        await chat.send("What is on today?")
        _ = await feed(stream, to: chat, through: { if case .runEnd = $0 { return true } else { return false } })
        client.failSends(with: .coreStopped(details: "x"))
        chat.composer = "And tomorrow?"
        await chat.send()
        XCTAssertNotNil(chat.composerError)
        client.failSends(with: nil)
        await chat.retry()
        XCTAssertNil(chat.composerError)
        XCTAssertEqual(chat.composer, "")
        XCTAssertEqual(chat.messages.filter { $0.role == .user }.map(\.text), ["What is on today?", "And tomorrow?"])
    }

    /// "Try again" under a cut-off reply, with nothing in the box, sends the last message again.
    func testTryAgainUnderACutOffReplySendsTheLastMessageAgain() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client, now: { self.now })
        let stream = client.events(since: nil)
        await chat.send("Draft a reply to Maria")
        _ = await feed(stream, to: chat, through: isApproval)
        let id = try XCTUnwrap(chat.currentSessionID)
        client.simulateCoreRestart()
        await chat.open(id: id)
        await chat.retry()
        XCTAssertTrue(chat.isRunning)
        XCTAssertEqual(chat.messages.filter { $0.role == .user }.map(\.text), ["Draft a reply to Maria", "Draft a reply to Maria"])
        await chat.stop()
    }

    func testAStopTheCoreRefusesIsShownAndTheReplyKeepsRunning() async throws {
        let client = try await makeClient()
        let chat = ChatStore(client: client, now: { self.now })
        let stream = client.events(since: nil)
        await chat.send("Draft a reply to Maria")
        _ = await feed(stream, to: chat, through: isApproval)
        client.failCancels(with: .failed("The core is busy."))
        await chat.stop()
        XCTAssertTrue(chat.isRunning, "nothing was stopped, so the reply still shows as running")
        XCTAssertFalse(chat.isStopping, "the Stop button can be pressed again")
        XCTAssertEqual(chat.composerError?.title, "Heron couldn't stop the reply")
        XCTAssertEqual(chat.composerError?.text, "The core is busy.")
        XCTAssertEqual(chat.composerError?.actions, [.stopReply])
        client.failCancels(with: nil)
        await chat.stop()
    }
}
