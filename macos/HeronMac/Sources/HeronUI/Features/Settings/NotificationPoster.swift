import Foundation
import UserNotifications

/// What the app needs from the notification center. A protocol so tests use a recorder: whether the real
/// `UNUserNotificationCenter` works from `swift test` is unverified, and this avoids the question.
public protocol NotificationPoster: Sendable {
    /// Asks macOS for permission to post. Returns whether it was granted.
    func requestAuthorization() async -> Bool
    /// Whether macOS currently lets the app post.
    func isAuthorized() async -> Bool
    /// Whether the user has refused notifications, so nothing can be posted and macOS will not ask again. Not the
    /// same as `!isAuthorized()`: before the first request the answer is "not yet", and a notice may still ask.
    func isDenied() async -> Bool
    func post(title: String, body: String, identifier: String) async throws
}

extension NotificationPoster {
    /// A poster that cannot tell is treated as not denied, so the core's notices are not handed over blindly.
    public func isDenied() async -> Bool { false }
}

/// The real center. `UNUserNotificationCenter.current()` is read only when a method runs, so building this in a
/// process with no app bundle (a test host) is harmless until something calls it.
public struct SystemNotificationPoster: NotificationPoster {
    public init() {}

    /// False in a test host or under `swift run`, where `UNUserNotificationCenter.current()` can raise: there
    /// nothing is asked or posted.
    private static var hasAppBundle: Bool { Bundle.main.bundleURL.pathExtension == "app" }

    public func requestAuthorization() async -> Bool {
        guard Self.hasAppBundle else { return false }
        return (try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])) ?? false
    }

    public func isAuthorized() async -> Bool {
        guard Self.hasAppBundle else { return false }
        let settings = await UNUserNotificationCenter.current().notificationSettings()
        switch settings.authorizationStatus {
        case .authorized, .provisional: return true
        default: return false
        }
    }

    public func isDenied() async -> Bool {
        // Without an app bundle nothing can ever be posted, so the core must keep posting its own notices.
        guard Self.hasAppBundle else { return true }
        return await UNUserNotificationCenter.current().notificationSettings().authorizationStatus == .denied
    }

    /// The request `post` hands to the center. It sets a title and a body and nothing else: no category and so no
    /// action buttons, which is how F3's notices never offer "approve" or "deny". Public so a test can check that.
    public static func request(title: String, body: String, identifier: String) -> UNNotificationRequest {
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        return UNNotificationRequest(identifier: identifier, content: content, trigger: nil)
    }

    public func post(title: String, body: String, identifier: String) async throws {
        guard Self.hasAppBundle else { return }
        try await UNUserNotificationCenter.current().add(Self.request(title: title, body: body, identifier: identifier))
    }
}
