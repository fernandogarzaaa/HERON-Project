import XCTest
import HeronCore
@testable import HeronUI

/// The sidebar's conversations: the right-click menu (Pin, Rename, Archive, Delete) and what the list shows.
@MainActor
final class SidebarConversationTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_795_003_200)

    private func makeChat() async throws -> (PreviewCoreClient, ChatStore) {
        let client = PreviewCoreClient(endpoint: CoreEndpoint(port: 1, token: "t", backgroundOwner: true, version: "test"), now: now)
        try await client.acceptRiskNotice(version: 3)
        return (client, ChatStore(client: client, now: { self.now }))
    }

    /// Starts a conversation and lets its reply finish. Returns its id.
    private func converse(_ chat: ChatStore, _ client: PreviewCoreClient, _ text: String) async throws -> String {
        chat.startNewConversation()
        let stream = client.events(since: nil)
        await chat.send(text)
        for await event in stream { chat.handle(event); if case .runEnd = event.kind { break } }
        await chat.loadSessions()
        return try XCTUnwrap(chat.currentSessionID)
    }

    func testANewConversationIsListedOnlyOnceItHasAMessage() async throws {
        let (client, chat) = try await makeChat()
        await chat.loadSessions()
        let before = chat.sessions.map(\.id)
        chat.startNewConversation()
        chat.startNewConversation()
        await chat.loadSessions()
        XCTAssertEqual(chat.sessions.map(\.id), before, "New conversation saves nothing until something is sent")
        let id = try await converse(chat, client, "remind me in 1 minute to test notifications")
        XCTAssertEqual(chat.sessions.first?.id, id)
        XCTAssertEqual(chat.sessions.first?.title, "remind me in 1 minute to test notificati…", "titled from its first message")
        XCTAssertFalse(chat.sessions.contains { $0.title.isEmpty })
    }

    func testPinnedConversationsComeFirst() async throws {
        let (client, chat) = try await makeChat()
        _ = try await converse(chat, client, "Prep my day")
        _ = try await converse(chat, client, "look at my downloads folder")
        let before = chat.sessions.map(\.id)
        let second = try XCTUnwrap(before.dropFirst().first)
        XCTAssertFalse(chat.sessions.contains { $0.pinned })
        await chat.setPinned(second, true)
        XCTAssertEqual(chat.sessions.first?.id, second)
        XCTAssertEqual(chat.sessions.first?.pinned, true)
        XCTAssertEqual(chat.sessions.filter(\.pinned).count, 1)
        await chat.setPinned(second, false)
        XCTAssertEqual(chat.sessions.map(\.id), before, "unpinned, it goes back to its place")
        XCTAssertNil(chat.sessionActionError)
    }

    func testRenameTrimsTheNameAndRefusesAnEmptyOne() async throws {
        let (client, chat) = try await makeChat()
        let id = try await converse(chat, client, "Prep my day")
        let renamed = await chat.rename(id, to: "  Morning plan  ")
        XCTAssertTrue(renamed)
        XCTAssertEqual(chat.sessions.first { $0.id == id }?.title, "Morning plan")
        XCTAssertEqual(chat.title, "Morning plan", "the open conversation's title follows")
        let refused = await chat.rename(id, to: "   ")
        XCTAssertFalse(refused)
        XCTAssertEqual(chat.sessionActionError, "A conversation needs a name.")
        XCTAssertEqual(chat.sessions.first { $0.id == id }?.title, "Morning plan")
        await chat.rename(id, to: "Morning")
        XCTAssertNil(chat.sessionActionError, "a later success clears the error")
    }

    func testArchiveHidesAConversationUntilItIsUnarchived() async throws {
        let (client, chat) = try await makeChat()
        let id = try await converse(chat, client, "Prep my day")
        await chat.setArchived(id, true)
        XCTAssertFalse(chat.sessions.contains { $0.id == id })
        XCTAssertFalse(chat.showsArchived)
        await chat.toggleArchived()
        XCTAssertTrue(chat.showsArchived)
        XCTAssertEqual(chat.archivedSessions.map(\.id), [id])
        XCTAssertEqual(chat.archivedSessions.first?.archived, true)
        await chat.setArchived(id, false)
        XCTAssertTrue(chat.sessions.contains { $0.id == id })
        XCTAssertTrue(chat.archivedSessions.isEmpty, "the open archived list refreshes too")
        await chat.toggleArchived()
        XCTAssertFalse(chat.showsArchived)
    }

    func testDeletingTheOpenConversationStartsANewOne() async throws {
        let (client, chat) = try await makeChat()
        let keep = try await converse(chat, client, "Prep my day")
        let gone = try await converse(chat, client, "look at my downloads folder")
        await chat.delete(gone)
        XCTAssertFalse(chat.sessions.contains { $0.id == gone })
        XCTAssertTrue(chat.sessions.contains { $0.id == keep })
        XCTAssertNil(chat.currentSessionID)
        XCTAssertTrue(chat.messages.isEmpty)
        do { _ = try await client.session(id: gone); XCTFail("deleted for good") } catch { XCTAssertEqual(error as? CoreError, .notFound) }
    }

    func testDeletingAnotherConversationKeepsTheOpenOne() async throws {
        let (client, chat) = try await makeChat()
        let other = try await converse(chat, client, "Prep my day")
        let open = try await converse(chat, client, "look at my downloads folder")
        await chat.delete(other)
        XCTAssertEqual(chat.currentSessionID, open)
        XCTAssertFalse(chat.messages.isEmpty)
    }

    /// The core refuses to delete a conversation while it replies: the reason shows, and nothing changes.
    func testAFailedDeleteSaysWhyAndKeepsTheConversation() async throws {
        let (client, chat) = try await makeChat()
        chat.startNewConversation()
        let stream = client.events(since: nil)
        await chat.send("Draft a reply to Maria")
        for await event in stream { chat.handle(event); if case .approval = event.kind { break } }
        let id = try XCTUnwrap(chat.currentSessionID)
        await chat.delete(id)
        XCTAssertEqual(chat.sessionActionError, "Stop the running reply before deleting this conversation.")
        XCTAssertEqual(chat.currentSessionID, id)
        XCTAssertTrue(chat.sessions.contains { $0.id == id })
        await chat.stop()
    }
}
