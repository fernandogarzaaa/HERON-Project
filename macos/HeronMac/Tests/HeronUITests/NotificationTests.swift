import XCTest
import HeronCore
@testable import HeronUI

@MainActor
final class NotificationTests: XCTestCase {
    private final class RecordingPoster: NotificationPosting {
        struct Post: Equatable { var title: String; var body: String; var identifier: String }
        var posts: [Post] = []
        func post(title: String, body: String, identifier: String) {
            posts.append(Post(title: title, body: body, identifier: identifier))
        }
    }

    private func makeModel(_ poster: RecordingPoster) -> AppModel {
        AppModel(client: PreviewCoreClient(),
                 preferences: DisplayPreferences(defaults: UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!),
                 userFirstName: "Sam", notifier: poster)
    }

    private func notification(seq: Int, _ title: String = "Morning brief", _ body: String = "Three meetings today.") -> CoreEvent {
        CoreEvent(seq: seq, sessionID: "", runID: "", kind: .notification(title: title, body: body))
    }

    func test_aNotificationEventPostsOnceWithItsTitleAndBody() {
        let poster = RecordingPoster()
        let model = makeModel(poster)
        model.route(notification(seq: 7))
        XCTAssertEqual(poster.posts.map(\.title), ["Morning brief"])
        XCTAssertEqual(poster.posts.map(\.body), ["Three meetings today."])
    }

    func test_theSameSeqDoesNotPostTwice() {
        let poster = RecordingPoster()
        let model = makeModel(poster)
        model.route(notification(seq: 7))
        model.route(notification(seq: 7))
        model.route(notification(seq: 6, "An older one"))
        XCTAssertEqual(poster.posts.count, 1)
    }

    func test_eachNewSeqPostsWithItsOwnIdentifier() {
        let poster = RecordingPoster()
        let model = makeModel(poster)
        model.route(notification(seq: 7))
        model.route(notification(seq: 8, "Digest", "Done."))
        XCTAssertEqual(poster.posts.count, 2)
        XCTAssertEqual(Set(poster.posts.map(\.identifier)).count, 2, "two events never share an identifier")
    }

    func test_aRestartedCoreNumbersFromOneAgainAndStillPosts() {
        let poster = RecordingPoster()
        let model = makeModel(poster)
        model.route(notification(seq: 40))
        model.route(CoreEvent(kind: .resync(oldest: 1)))
        model.route(notification(seq: 1, "After restart", "Still shown."))
        XCTAssertEqual(poster.posts.map(\.title), ["Morning brief", "After restart"])
        XCTAssertEqual(Set(poster.posts.map(\.identifier)).count, 2, "seq 1 of the new core does not replace an earlier banner")
    }

    func test_aNotificationDoesNotTouchTheConversationOrTheAvatar() {
        let poster = RecordingPoster()
        let model = makeModel(poster)
        model.route(notification(seq: 1))
        XCTAssertTrue(model.chat.messages.isEmpty)
        XCTAssertEqual(model.avatar.state, .idle)
    }
}
